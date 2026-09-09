//! One drum hit, synthesised — the primitive the built-in drum machine is made
//! of.
//!
//! > *"a built in general purpose drum machine that can just make a variety of
//! > drum styles and sounds."*
//!
//! # Why synthesis and not samples
//!
//! A sampled kit is a folder of files, and Fontelle's whole position is that a
//! file supplies *defaults* you then own (TDD §1.1). A synthesised kit is the
//! same position taken to the drums: every hit is a handful of numbers, so
//! every hit is tunable, every kit is a table rather than sixty megabytes, and
//! twenty style presets cost twenty rows instead of twenty download links.
//! It also means the drum machine works on a fresh install with no bank
//! configured, which nothing else in this program does yet.
//!
//! # The shape of a drum
//!
//! Every model here is the same three parts in different proportions, which is
//! what makes one struct enough for a kick and a cymbal:
//!
//! - a **body**: a pitched oscillator whose pitch falls (the *bend*) and whose
//!   level falls (the *decay*). On a kick that is the whole sound; on a hat it
//!   is nothing.
//! - a **noise** half, through a filter whose corner is the *tone*. On a hat
//!   that is the whole sound; on a kick it is the beater.
//! - a **snap**: a very short burst at the front, which is the click your ear
//!   uses to tell a kick from a bass note.
//!
//! [`DrumModel`] then says how those three are wired — a snare's noise is
//! band-passed and its body is two detuned tones, a clap's noise stutters, a
//! cymbal's decays in two stages. The rest is the same arithmetic.
//!
//! # RT rules
//!
//! INVARIANT 1: no allocation, no branching on anything that is not already in
//! a register. [`DrumSynth`] is `Copy` and fixed-size so a voice can hold one
//! per layer slot, and everything expensive — the exponentials behind the
//! decay coefficients — is computed once in [`DrumSynth::trigger`], which runs
//! on note-on rather than per sample.

use crate::filter::{SvfFilter, SvfMode};
use crate::modal::{ModalBank, ModalMode};
use crate::oscillator::{OscKind, Oscillator};

/// Which drum a voice is.
///
/// Not a preset — a **topology**. Two kits' kicks are the same `Kick` with
/// different numbers, and that is the point: the model decides what is wired
/// to what, and [`DrumVoice`] decides how it sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum DrumModel {
    /// A falling pitched body with a beater click. The bend is the sound.
    #[default]
    Kick,
    /// Two detuned bodies and a band of noise on top — the noise is what makes
    /// the snares rattle, and the two tones are the head and the shell.
    Snare,
    /// A pitched body with a gentler bend and almost no noise. A kick with the
    /// bend backed off is a tom, which is exactly what one is.
    Tom,
    /// Noise through a high-pass, cut short. No body at all.
    ClosedHat,
    /// The same, held open. Its own model rather than a long `ClosedHat`
    /// because a kit wants both on two keys with two decays.
    OpenHat,
    /// Noise in four bursts and then a tail. The stutter is the whole
    /// character — a clap without it is a short snare.
    Clap,
    /// Broadband noise with a long two-stage tail: a fast edge and a wash.
    Cymbal,
    /// A very short pitched crack with noise over it. Rimshots and sticks.
    Rim,
    /// Two square tones through a narrow band. Cowbells, blocks, agogos —
    /// everything metallic and pitched.
    Cowbell,
    /// A short clean pitched blip. Congas, bongos, timbales, toms in a hurry.
    Perc,
}

impl DrumModel {
    /// Every model, in the order a kit lays them out: the three that make a
    /// beat, then the rest.
    pub const ALL: [Self; 10] = [
        Self::Kick,
        Self::Snare,
        Self::ClosedHat,
        Self::OpenHat,
        Self::Tom,
        Self::Clap,
        Self::Cymbal,
        Self::Rim,
        Self::Cowbell,
        Self::Perc,
    ];

    /// What the panel calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Kick => "Kick",
            Self::Snare => "Snare",
            Self::Tom => "Tom",
            Self::ClosedHat => "Closed hat",
            Self::OpenHat => "Open hat",
            Self::Clap => "Clap",
            Self::Cymbal => "Cymbal",
            Self::Rim => "Rim",
            Self::Cowbell => "Cowbell",
            Self::Perc => "Perc",
        }
    }

    /// Which way the noise half is filtered.
    ///
    /// A hat wants everything above its corner and a snare wants a band around
    /// it: the same noise through two different filters is the difference
    /// between a rattle and a hiss.
    fn noise_mode(self) -> SvfMode {
        match self {
            Self::Snare | Self::Clap | Self::Cowbell | Self::Rim => SvfMode::Bandpass,
            _ => SvfMode::Highpass,
        }
    }

    /// How resonant that filter is. A narrow band is a *pitch* in the noise,
    /// which is what a cowbell is and what a cymbal must not be.
    fn noise_q(self) -> f32 {
        match self {
            Self::Cowbell => 8.0,
            Self::Snare | Self::Rim => 1.4,
            Self::Clap => 2.0,
            _ => 0.7,
        }
    }

    /// **The shape of the thing being struck**, as the modes it rings at.
    ///
    /// This is the difference between the drums, and it is a fact about the
    /// object rather than about the kit: every kick is a head over a deep
    /// shell whatever an 808 or a 909 did to it, so the ratios live on the
    /// model and the tuning lives on the voice.
    ///
    /// The numbers are the physics where the physics is known and the
    /// instrument where it is not. An ideal circular membrane rings at 1.000,
    /// 1.593, 2.135, 2.295, 2.917 — those are Bessel zeros and they are not
    /// negotiable. A real drum is a membrane *loaded by the air inside a
    /// shell*, which pulls the low modes together and is why a tuned tom
    /// sounds like a note and an untuned one does not; that is the second set.
    /// A cowbell and a rim are bars rather than membranes and ring at a bar's
    /// ratios, which are much further apart. A cymbal has so many modes that
    /// naming five would be a lie, so it has none and stays noise.
    fn modes(self) -> &'static [ModalMode] {
        match self {
            // A kick is a head over a long column of air: the fundamental
            // carries nearly everything and the modes above it are what makes
            // the front of the hit read as a skin rather than as a tone.
            Self::Kick => &[
                ModalMode {
                    ratio: 1.0,
                    decay: 1.0,
                    gain: 1.0,
                },
                ModalMode {
                    ratio: 1.59,
                    decay: 0.28,
                    gain: 0.22,
                },
                ModalMode {
                    ratio: 2.14,
                    decay: 0.18,
                    gain: 0.12,
                },
                ModalMode {
                    ratio: 2.30,
                    decay: 0.14,
                    gain: 0.08,
                },
            ],
            // A tom is the membrane everybody pictures, air-loaded: the first
            // few modes pulled toward whole ratios, which is what makes a tom
            // nearly a note and never quite one.
            Self::Tom => &[
                ModalMode {
                    ratio: 1.0,
                    decay: 1.0,
                    gain: 1.0,
                },
                ModalMode {
                    ratio: 1.50,
                    decay: 0.62,
                    gain: 0.55,
                },
                ModalMode {
                    ratio: 1.75,
                    decay: 0.48,
                    gain: 0.38,
                },
                ModalMode {
                    ratio: 2.00,
                    decay: 0.36,
                    gain: 0.26,
                },
                ModalMode {
                    ratio: 2.44,
                    decay: 0.25,
                    gain: 0.16,
                },
                ModalMode {
                    ratio: 2.89,
                    decay: 0.18,
                    gain: 0.10,
                },
            ],
            // A snare is a tom with the modes damped hard by the wires and by
            // the second head: short, and the shell's own ring over it.
            Self::Snare => &[
                ModalMode {
                    ratio: 1.0,
                    decay: 0.55,
                    gain: 1.0,
                },
                ModalMode {
                    ratio: 1.50,
                    decay: 0.35,
                    gain: 0.60,
                },
                ModalMode {
                    ratio: 1.87,
                    decay: 0.26,
                    gain: 0.45,
                },
                ModalMode {
                    ratio: 2.41,
                    decay: 0.18,
                    gain: 0.30,
                },
                ModalMode {
                    ratio: 3.60,
                    decay: 0.12,
                    gain: 0.18,
                },
            ],
            // Congas and bongos: a small head at high tension, so the modes
            // are close to the ideal membrane's and ring longer than a tom's.
            Self::Perc => &[
                ModalMode {
                    ratio: 1.0,
                    decay: 1.0,
                    gain: 1.0,
                },
                ModalMode {
                    ratio: 1.593,
                    decay: 0.70,
                    gain: 0.45,
                },
                ModalMode {
                    ratio: 2.135,
                    decay: 0.52,
                    gain: 0.28,
                },
                ModalMode {
                    ratio: 2.917,
                    decay: 0.34,
                    gain: 0.15,
                },
            ],
            // A **bar**, not a membrane: a rim, a block, a stick. Bar modes
            // are far apart (roughly 1 : 2.76 : 5.40), which is exactly why a
            // woodblock reads as wood and not as a drum.
            Self::Rim => &[
                ModalMode {
                    ratio: 1.0,
                    decay: 1.0,
                    gain: 1.0,
                },
                ModalMode {
                    ratio: 2.76,
                    decay: 0.45,
                    gain: 0.40,
                },
                ModalMode {
                    ratio: 5.40,
                    decay: 0.22,
                    gain: 0.16,
                },
            ],
            // A cowbell is two bars welded together — the two tones it has
            // always had, plus the partials that make it clang.
            Self::Cowbell => &[
                ModalMode {
                    ratio: 1.0,
                    decay: 1.0,
                    gain: 1.0,
                },
                ModalMode {
                    ratio: 1.48,
                    decay: 0.90,
                    gain: 0.85,
                },
                ModalMode {
                    ratio: 2.76,
                    decay: 0.40,
                    gain: 0.30,
                },
                ModalMode {
                    ratio: 3.98,
                    decay: 0.25,
                    gain: 0.18,
                },
            ],
            // Hats, cymbals and claps have no pitched half to give modes to.
            // A cymbal *is* modal, with hundreds of them, and five would read
            // as a chime rather than as a cymbal — so it stays noise, which
            // is the honest approximation.
            Self::ClosedHat | Self::OpenHat | Self::Cymbal | Self::Clap => &[],
        }
    }
}

/// Which waveform the pitched half is.
///
/// A sine is the drum-machine body everybody knows; a triangle is a little
/// harder; a square is a chiptune kit's whole character, and having it here is
/// what lets that be a preset rather than a second synthesiser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum DrumBody {
    #[default]
    Sine,
    Triangle,
    Square,
}

impl DrumBody {
    pub const ALL: [Self; 3] = [Self::Sine, Self::Triangle, Self::Square];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sine => "Sine",
            Self::Triangle => "Triangle",
            Self::Square => "Square",
        }
    }

    fn osc(self) -> OscKind {
        match self {
            Self::Sine => OscKind::Sine,
            Self::Triangle => OscKind::Triangle,
            Self::Square => OscKind::Square,
        }
    }
}

/// One hit's settings — the whole of what makes a kick a kick.
///
/// Plain numbers with `serde` on them, because a kit is a table of these and a
/// saved patch is that table written out. Every field is clamped where it is
/// read rather than where it is set: a project file is something a person may
/// edit by hand, and a tune of zero should be a quiet drum rather than a
/// division by zero on the audio thread.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrumVoice {
    pub model: DrumModel,
    pub body: DrumBody,
    /// Where the pitched half settles, in hertz.
    pub tune_hz: f32,
    /// How far above that it starts, in semitones. Zero is a flat tone.
    pub bend_semitones: f32,
    /// How long the fall takes, in seconds.
    pub bend_s: f32,
    /// How long the whole hit lasts, in seconds — the time it takes to fall by
    /// [`DECAY_SPAN_DB`].
    pub decay_s: f32,
    /// The noise filter's corner, in hertz. The brightness control.
    pub tone_hz: f32,
    /// The balance between the body and the noise, 0..=1. Zero is a pure tone
    /// and one is pure noise.
    pub noise: f32,
    /// The click at the front, 0..=1.
    pub snap: f32,
    /// Saturation, 0..=1. Louder and flatter, which is what a drum bus is for.
    pub drive: f32,
    /// This hit's level against the others in the kit.
    pub gain_db: f32,
    /// How metallic the noise half is, 0..=1.
    ///
    /// Zero is white noise. One is the six square oscillators the 808 makes
    /// its hats and cymbals from — at ratios that share no harmonics, so the
    /// sum never settles into a pitch and reads as metal — tuned from
    /// `tune_hz`, which is what lets a 606's hats sit above an 808's rather
    /// than being the same hat twice.
    ///
    /// > *"the drumkits in the drum machine kind of all sound very similar"*
    ///
    /// Measured, and true: with white noise through one filter as the only
    /// noise there was, every kit's hat came out between 10 and 13 kHz and
    /// 25 ms long. The *source* is what tells the machines apart, and this is
    /// the knob that changes it. Read with a default so a kit written before
    /// the knob existed sounds exactly as it did.
    #[serde(default)]
    pub metal: f32,
    /// Sample-rate and bit-depth reduction, 0..=1.
    ///
    /// The LinnDrum, the SP-1200 and every chiptune got their character from
    /// a converter rather than a circuit, and a lo-fi kit is that converter.
    /// At one the rate is a thirteenth of the host's and the depth is four
    /// bits; at zero it is not there.
    #[serde(default)]
    pub crush: f32,
    /// How much of the pitched half is a **struck membrane** rather than an
    /// oscillator, 0..=1.
    ///
    /// > *"the sounds ... still sound way too synthesized and not realistic
    /// > enough ... ultimately still just sounding like tweaked versions of
    /// > the same synthesized sounding sounds."*
    ///
    /// Measured, and the cause was the model rather than the numbers: every
    /// pitched hit was one oscillator, so a kick, a tom and a conga were the
    /// same sine with three envelopes on it and no setting of tune, bend or
    /// decay could make any of them ring like a drum. A real head rings at a
    /// set of **inharmonic** modes — 1.00, 1.59, 2.14, 2.30, 2.92 of its
    /// fundamental for an ideal circular membrane — each dying at its own
    /// rate, the high ones first. At one this is that bank
    /// ([`ModalBank`](crate::ModalBank)), struck by the hit's own transient,
    /// with the ratios read off the model. At zero it is the oscillator that
    /// was always here, so every kit written before this knob existed sounds
    /// exactly as it did.
    #[serde(default)]
    pub modes: f32,
    /// A second, slower decay under the first, 0..=1.
    ///
    /// A real drum does not fade at one rate: the strike dies fast and the
    /// shell keeps ringing under it. One exponential cannot be both, and a hit
    /// that is only the first is the thing that reads as "a sample of a drum
    /// machine" rather than as a drum in a room.
    #[serde(default)]
    pub tail: f32,
    /// How much the noise half rings rather than hisses, 0..=1.
    ///
    /// A snare's wires are not a band of white noise: they are a band of white
    /// noise buzzing against a shell that has a note. This rings the filtered
    /// noise through a resonance a fifth over the body, which is where a
    /// snare's rattle sits and what the ear reads as wires rather than as air.
    #[serde(default)]
    pub rattle: f32,
}

impl Default for DrumVoice {
    /// A plain kick — the identity a kit builder starts from.
    fn default() -> Self {
        Self {
            model: DrumModel::Kick,
            body: DrumBody::Sine,
            tune_hz: 55.0,
            bend_semitones: 24.0,
            bend_s: 0.04,
            decay_s: 0.3,
            tone_hz: 500.0,
            noise: 0.05,
            snap: 0.3,
            drive: 0.15,
            gain_db: 0.0,
            metal: 0.0,
            crush: 0.0,
            modes: 0.0,
            tail: 0.0,
            rattle: 0.0,
        }
    }
}

/// The 808's six hat oscillators, as ratios over the lowest.
///
/// 205, 304, 370, 523, 540 and 800 Hz on the original — chosen, or arrived
/// at, so that no two share a harmonic, which is why the sum never fuses into
/// a note. Scaled by the hit's `tune_hz` rather than fixed, so a kit can put
/// its hats where it likes.
const METAL_RATIOS: [f32; 6] = [1.0, 1.483, 1.800, 2.546, 2.630, 3.897];

/// How much the bank is turned up against white noise, after the filter.
///
/// A square wave's harmonics fall as 1/n, so above a hat's corner a bank of
/// them carries about a tenth of the power a flat noise does — measured at
/// ten decibels, with the six summed to unit RMS. This is the make-up that
/// lets `metal` sweep between the two at one level rather than fading out
/// in the middle, and it is applied to the bank's *filtered* half so the
/// click at the front, which is the unfiltered noise, is not made-up too.
const METAL_LEVEL: f32 = 7.5;

/// The coarsest `crush` goes: the host rate divided by this, and this many
/// bits fewer than sixteen. Thirteen and twelve put full crush at about
/// 3.7 kHz and four bits at 48 kHz — the low end of what an eight-bit machine
/// did, and past it there is only a click.
const CRUSH_MAX_DIVISOR: f32 = 12.0;
const CRUSH_MAX_BITS_LOST: f32 = 12.0;

/// How much the struck bank is turned up against the oscillator it replaces.
///
/// A resonator excited by a burst arrives far under an oscillator running at
/// full amplitude: the bank's input gain is scaled by `(1 − r)` so its modes
/// balance against each other, and that leaves the sum an order of magnitude
/// down. Measured at about eighteen decibels with a tom's six modes, so this
/// is the make-up that lets `modes` sweep between the two at one level rather
/// than fading out in the middle — the same argument, and the same fix, as
/// [`METAL_LEVEL`].
const MODAL_LEVEL: f32 = 8.0;

/// How far a hit falls over its `decay_s` before it is called finished.
///
/// Sixty decibels: the standard "RT60" reading of a decay time, and the point
/// at which a hit is a thousandth of its peak and inaudible under anything
/// else. Named because the synth and any test of it have to agree.
pub const DECAY_SPAN_DB: f32 = 60.0;

/// Below this the voice reports itself finished and stops costing anything.
///
/// -100 dBFS, a full 40 dB under where the decay is already inaudible: the
/// margin is there because a voice freed a fraction too early is a tail cut
/// off, which is audible, and one freed late costs a few hundred samples of
/// arithmetic, which is not.
const SILENCE: f32 = 1.0e-5;

impl DrumVoice {
    /// The pitched half's frequency, as a positive number a phase can be
    /// advanced by. A hand-edited tune of zero or a negative one reads as the
    /// floor rather than as a stuck or backwards oscillator.
    fn base_hz(&self) -> f32 {
        self.tune_hz.clamp(10.0, 18_000.0)
    }

    fn amount(value: f32) -> f32 {
        if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// One drum voice, sounding.
///
/// `Copy` and fixed-size, so `fontelle_core`'s per-layer slot can hold one
/// without an allocation and a voice can be handed back to the pool by being
/// overwritten. Everything expensive is done in [`trigger`](Self::trigger).
#[derive(Debug, Clone, Copy)]
pub struct DrumSynth {
    body: Oscillator,
    /// The second body, for the models that have one — a snare's shell, a
    /// cowbell's other tone.
    body2: Oscillator,
    noise: Oscillator,
    /// The metallic noise source: six squares at [`METAL_RATIOS`], through
    /// a filter of their own so the two sources can be balanced *after* the
    /// corner, where the ear hears them.
    bank: [Oscillator; 6],
    bank_filter: SvfFilter,
    /// The sample `crush` is holding, and for how many more samples.
    held: f32,
    hold_left: u32,
    filter: SvfFilter,
    /// The level envelopes, held as their current value and a per-sample
    /// multiplier: an exponential decay is one multiply per sample, which is
    /// what the RT thread can afford.
    body_env: f32,
    body_decay: f32,
    noise_env: f32,
    noise_decay: f32,
    snap_env: f32,
    snap_decay: f32,
    /// The pitch fall, the same way. It runs from one to zero and the bend is
    /// scaled by it.
    bend_env: f32,
    bend_decay: f32,
    /// The struck membrane (`modes`), and the short burst that strikes it.
    ///
    /// A resonator has to be *hit* with something. A single impulse rings a
    /// bank cleanly but reads as a plucked string; a few milliseconds of noise
    /// under an envelope is a beater or a stick meeting a head, which is what
    /// this is.
    membrane: ModalBank,
    strike_env: f32,
    strike_decay: f32,
    /// The second, slower fall under the first (`tail`).
    tail_env: f32,
    tail_decay: f32,
    /// The noise's own resonance (`rattle`) — a snare's wires against a shell.
    rattle_filter: SvfFilter,
    /// Seconds since the trigger, for the models whose shape depends on where
    /// in the hit they are — a clap's stutter and a cymbal's second stage.
    t: f32,
    /// Set once the whole thing is under [`SILENCE`].
    done: bool,
}

impl Default for DrumSynth {
    fn default() -> Self {
        Self::new()
    }
}

impl DrumSynth {
    /// A voice that is not sounding. **Silent until triggered** — voices come
    /// out of a pool, and one that made a sound before anybody hit it would be
    /// a drum machine that plays itself.
    pub fn new() -> Self {
        Self {
            body: Oscillator::new(),
            body2: Oscillator::new(),
            noise: Oscillator::new(),
            // Spread through the cycle, or the six all step to +1 together
            // on the first sample and the hat opens with a click six times
            // the size of any one of them.
            bank: std::array::from_fn(|i| Oscillator::at_phase(i as f32 / 6.0)),
            bank_filter: SvfFilter::default(),
            held: 0.0,
            hold_left: 0,
            filter: SvfFilter::default(),
            body_env: 0.0,
            body_decay: 0.0,
            noise_env: 0.0,
            noise_decay: 0.0,
            snap_env: 0.0,
            snap_decay: 0.0,
            bend_env: 0.0,
            bend_decay: 0.0,
            membrane: ModalBank::new(),
            strike_env: 0.0,
            strike_decay: 0.0,
            tail_env: 0.0,
            tail_decay: 0.0,
            rattle_filter: SvfFilter::default(),
            t: 0.0,
            done: true,
        }
    }

    /// Whether the hit has finished and the slot can be reused.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Starts the hit. **Every piece of state is written here**, so a voice out
    /// of the pool carries nothing from the note before it — the same rule
    /// `Voice::trigger_note` keeps for filter memory and oscillator phase, and
    /// for the same reason: a retrigger is a fresh hit, not a hit added to
    /// whatever was still ringing.
    pub fn trigger(&mut self, voice: &DrumVoice, sample_rate: f32) {
        *self = Self::new();
        self.done = false;
        let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            // Nothing can be worked out per sample without a rate. The voice
            // is left triggered but with decays that finish immediately, which
            // renders silence rather than a division by zero.
            self.done = true;
            return;
        };

        // A decay is the multiplier that takes a level from one to
        // `10^(-DECAY_SPAN_DB/20)` over `decay_s` seconds. Worked out once,
        // here, because `powf` per sample is exactly what an RT thread cannot
        // spend.
        let coeff = |seconds: f32| {
            let seconds = if seconds.is_finite() {
                seconds.max(0.001)
            } else {
                0.001
            };
            let samples = (seconds * sr).max(1.0);
            10f32.powf(-DECAY_SPAN_DB / (20.0 * samples))
        };

        let model = voice.model;
        let decay = if voice.decay_s.is_finite() {
            voice.decay_s.clamp(0.002, 8.0)
        } else {
            0.3
        };

        self.body_env = 1.0;
        self.body_decay = coeff(decay);
        // A hat's noise is the hit and dies with it; a kick's beater is a
        // fraction of the length of the body. One number, read off the model,
        // rather than a second decay knob nobody would know how to set.
        self.noise_env = 1.0;
        self.noise_decay = coeff(match model {
            DrumModel::ClosedHat | DrumModel::OpenHat | DrumModel::Cymbal | DrumModel::Clap => {
                decay
            }
            DrumModel::Snare => decay * 0.9,
            _ => decay * 0.35,
        });
        // The click is always short — that is what makes it a click. A snap
        // that lasted as long as the hit would just be more noise.
        self.snap_env = DrumVoice::amount(voice.snap);
        self.snap_decay = coeff(0.004);

        self.bend_env = 1.0;
        self.bend_decay = coeff(if voice.bend_s.is_finite() {
            voice.bend_s.clamp(0.001, 2.0)
        } else {
            0.04
        });

        // --- the struck membrane (§`DrumVoice::modes`) ------------------
        //
        // Tuned once here, because two transcendentals a mode is exactly the
        // kind of thing that does not belong per sample. The bank rings for
        // the hit's own decay, and each mode scales that by its own number,
        // so a tom's high modes die first the way a real head's do.
        let modes = DrumVoice::amount(voice.modes);
        if modes > 0.0 {
            self.membrane.set(model.modes(), voice.base_hz(), decay, sr);
        }
        // The strike: a few milliseconds, and shorter for the hard-struck
        // models. A beater on a kick head is in contact for longer than a
        // stick on a rim, and the difference is audible as how much of the
        // bank's top end gets excited.
        self.strike_env = 1.0;
        self.strike_decay = coeff(match model {
            DrumModel::Rim | DrumModel::Cowbell => 0.0015,
            DrumModel::Snare | DrumModel::Perc => 0.003,
            _ => 0.006,
        });

        // The slow half. Four times the hit's own decay — long enough to be a
        // ring under it rather than a second hit, short enough that a kit does
        // not turn into a wash when every voice has some.
        self.tail_env = 1.0;
        self.tail_decay = coeff(decay * 4.0);

        self.t = 0.0;
    }

    /// One sample, then advance.
    ///
    /// Returns zero for ever once the hit has finished, so a caller that has
    /// not noticed [`is_done`](Self::is_done) gets silence rather than a stuck
    /// tone.
    pub fn next_sample(&mut self, voice: &DrumVoice, sample_rate: f32) -> f32 {
        if self.done {
            return 0.0;
        }
        let sr = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            return 0.0;
        };
        let nyquist = sr * 0.5;
        let model = voice.model;

        // --- the pitched half -------------------------------------------
        //
        // The bend is applied in semitones so it means the same thing at
        // 50 Hz as at 800 — an octave is an octave, which a linear offset in
        // hertz would not give.
        let bend = if voice.bend_semitones.is_finite() {
            voice.bend_semitones.clamp(-48.0, 48.0)
        } else {
            0.0
        };
        let base = voice.base_hz();
        let freq = (base * 2f32.powf(bend * self.bend_env / 12.0)).clamp(0.0, nyquist);
        let mut body = self.body.next_sample(voice.body.osc(), freq, sr);
        // The second tone, a fifth above and quieter: a snare's shell against
        // its head, a cowbell's two bars. Detuned rather than harmonic, so the
        // two beat against each other instead of fusing into one pitch.
        if matches!(model, DrumModel::Snare | DrumModel::Cowbell) {
            let second = (freq * 1.48).clamp(0.0, nyquist);
            body = body * 0.65 + self.body2.next_sample(voice.body.osc(), second, sr) * 0.5;
        }
        body *= self.body_env;
        // **The struck membrane.** At `modes` = 1 the pitched half is the
        // resonator bank rather than the oscillator: the same note, ringing at
        // the shape's own inharmonic modes with the high ones dying first,
        // which is the whole of what a sine cannot do. Crossfaded rather than
        // switched, so a kit can sit anywhere between the machine and the
        // instrument — an 808 kick is *meant* to be a sine.
        let modes = DrumVoice::amount(voice.modes);
        if modes > 0.0 {
            // Struck by a burst of the noise source under a very short
            // envelope: a beater, not an impulse. Using the same `raw` the
            // rest of the voice uses costs nothing and ties the strike to the
            // hit's own character.
            let strike = self.strike_env * self.noise.next_sample(OscKind::Noise, 0.0, sr);
            // Plus the front of the body itself, so a bank on a kick is
            // pushed by the beater *and* by the pitch drop that follows it.
            let rung = self
                .membrane
                .next_sample(strike + body * 0.25 * self.strike_env);
            body = body * (1.0 - modes) + rung * modes * MODAL_LEVEL;
        }
        // The slow half. Added under the fast one rather than replacing it, so
        // `tail` lengthens a hit without changing what the front of it sounds
        // like — which is what "the shell is still ringing" means.
        let tail = DrumVoice::amount(voice.tail);
        if tail > 0.0 {
            body += body * (self.tail_env / self.body_env.max(1e-6)).min(8.0) * tail * 0.35;
        }

        // --- the noise half ---------------------------------------------
        //
        // White noise, six squares, or a blend of the two — each through the
        // same filter, mixed after it. The bank is only advanced when it is
        // heard: six oscillators and a filter per sample on a kick with no
        // metal in it is a cost with no sound attached.
        let raw = self.noise.next_sample(OscKind::Noise, 0.0, sr);
        let corner = if voice.tone_hz.is_finite() {
            voice.tone_hz.clamp(20.0, nyquist * 0.98)
        } else {
            1_000.0
        };
        let coeffs = SvfFilter::coeffs(model.noise_mode(), corner, model.noise_q(), 0.0, sr);
        let metal = DrumVoice::amount(voice.metal);
        let mut noise = self.filter.process(raw, &coeffs) * (1.0 - metal);
        if metal > 0.0 {
            let mut sum = 0.0;
            for (osc, ratio) in self.bank.iter_mut().zip(METAL_RATIOS) {
                let hz = (base * ratio).min(nyquist);
                sum += osc.next_sample(OscKind::Square, hz, sr);
            }
            // Six unit squares sum to a little under unit RMS.
            let bank = sum * (1.0 / 6.0);
            noise += self.bank_filter.process(bank, &coeffs) * metal * METAL_LEVEL;
        }
        // **The rattle.** The filtered noise rung through a resonance a fifth
        // over the body, which is where a snare's wires buzz against its
        // shell. Added rather than replacing, so the knob is "how much of this
        // is wires" and not "swap the noise for a whistle".
        let rattle = DrumVoice::amount(voice.rattle);
        if rattle > 0.0 {
            let hz = (base * 1.5).clamp(60.0, nyquist * 0.9);
            let ring = SvfFilter::coeffs(SvfMode::Bandpass, hz, 6.0, 0.0, sr);
            noise += self.rattle_filter.process(noise, &ring) * rattle * 1.4;
        }
        noise *= self.noise_env;
        // A clap is four bursts and then a tail: the stutter is the whole
        // character, and without it a clap is a short snare. Gated on `t`
        // rather than on a fourth envelope, because the gaps are silence
        // rather than a decay.
        if model == DrumModel::Clap {
            noise *= clap_gate(self.t);
        }
        // A cymbal's wash: a second, much slower fall under the first, so the
        // edge dies away and the tail keeps going.
        if model == DrumModel::Cymbal {
            noise += self.filter.process(raw, &coeffs) * self.body_env * 0.35;
        }

        // --- mix ---------------------------------------------------------
        //
        // The balance is equal-power, so sweeping it does not dip in the
        // middle — the same reason the crossfade between two clips is.
        let n = DrumVoice::amount(voice.noise);
        let (body_gain, noise_gain) = match model {
            // These have no pitched half at all. Forcing the balance here
            // rather than asking every kit to write `noise: 1.0` is what keeps
            // a hat a hat when somebody turns the knob.
            DrumModel::ClosedHat | DrumModel::OpenHat | DrumModel::Cymbal | DrumModel::Clap => {
                (0.0, 1.0)
            }
            _ => {
                let angle = n * std::f32::consts::FRAC_PI_2;
                (angle.cos(), angle.sin())
            }
        };
        let snap = self.snap_env * raw * 0.7;
        let mut out = body * body_gain + noise * noise_gain + snap;

        // Saturation: `tanh` in all but name, and cheap. Louder and flatter
        // for the same peak, which is what a drum bus does and what makes an
        // 808 kick sit in a mix.
        let drive = DrumVoice::amount(voice.drive);
        if drive > 0.0 {
            let amount = 1.0 + drive * 8.0;
            out = (out * amount).tanh() / (1.0 + drive * 1.6);
        }

        // The converter. A held sample is the rate reduction and a rounded
        // one is the depth, done together because that is what an eight-bit
        // machine did: one converter, both limits. After the drive, so the
        // saturation is what gets crushed rather than the other way round,
        // which is the order the signal went through the hardware.
        let crush = DrumVoice::amount(voice.crush);
        if crush > 0.0 {
            if self.hold_left == 0 {
                let bits = 16.0 - crush * CRUSH_MAX_BITS_LOST;
                let step = 2f32.powf(1.0 - bits);
                self.held = (out / step).round() * step;
                self.hold_left = 1 + (crush * CRUSH_MAX_DIVISOR) as u32;
            }
            self.hold_left -= 1;
            out = self.held;
        }

        let gain_db = if voice.gain_db.is_finite() {
            voice.gain_db.clamp(-60.0, 12.0)
        } else {
            0.0
        };
        out *= 10f32.powf(gain_db / 20.0);

        // --- advance -----------------------------------------------------
        self.body_env *= self.body_decay;
        self.noise_env *= self.noise_decay;
        self.snap_env *= self.snap_decay;
        self.bend_env *= self.bend_decay;
        self.strike_env *= self.strike_decay;
        self.tail_env *= self.tail_decay;
        self.t += 1.0 / sr;
        // A hit with a tail on it is not finished when its fast half is: the
        // slow one is still ringing, and freeing the voice there would cut the
        // ring off mid-note. Checked against the tail only when there is one,
        // so a hit without one frees itself exactly as early as it always did.
        let ringing = DrumVoice::amount(voice.tail) > 0.0 && self.tail_env >= SILENCE;
        if self.body_env < SILENCE && self.noise_env < SILENCE && !ringing {
            self.done = true;
        }

        // The last line of defence. Everything above is bounded by
        // construction, but `voice` comes off a project file: one NaN reaching
        // the bus poisons every sample after it, right through the master.
        if out.is_finite() {
            out.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }
}

/// A clap's stutter: three fast repeats and then the tail.
///
/// The numbers are the ones a 909 uses and everybody has copied since — about
/// ten milliseconds apart, each burst a little quieter, then the body of the
/// noise from the fourth on. Written as a function of time rather than as a
/// counter so that the shape does not depend on the block size.
fn clap_gate(t: f32) -> f32 {
    const SPACING: f32 = 0.010;
    const BURSTS: f32 = 3.0;
    if t >= SPACING * BURSTS {
        return 1.0;
    }
    // Where in the current burst we are, 0..1 — each burst decays across its
    // own gap, which is what makes it read as four hits rather than a tremolo.
    let within = (t / SPACING).fract();
    let index = (t / SPACING).floor();
    (1.0 - within * 0.85) * (1.0 - index * 0.15).max(0.2)
}
