//! Flopsynth's factory bank (`docs/flopsynth-plan.md` §7).
//!
//! > *"should have lots of built in presets in a bank for tons of instruments
//! > organized by type. make sure to include a wide variety between synthesis
//! > and synths that sound like other instruments like choir ahhs or strings
//! > etc."* — Ty, 2026-09-06
//!
//! # Recipes, not files
//!
//! What *ships* is files (§P.3): one JSON per preset under
//! `assets/presets/flopsynth/<category>/<name>.json`, embedded in the binary
//! and loaded through the same bank every other device uses. What is **here**
//! is the code that writes those files, because two hundred and ten JSON
//! documents of two hundred fields each are not reviewable and a table of
//! rows is. `cargo xtask export-factory-presets` turns this into that, and a
//! test holds that every committed file still equals its row — so the recipe
//! stays the reviewable truth and the file stays the thing that ships.
//!
//! # The rules every row follows (§7.3)
//!
//! Written down because the drum kits taught that a bank of "different"
//! presets can be one preset at forty brightnesses (`drum-kit-axes`):
//!
//! - **Imitations differ on the source and the filter model**, not only on
//!   envelope times. Strings are a saw stack through a *clean* low-pass with
//!   ensemble; choirs are a stack through the *formant* filter; brass is a saw
//!   through a *ladder* with an overshooting envelope; bells and e-pianos are
//!   *FM* tables.
//! - **Vibrato arrives late and fades in** on every acoustic imitation. A
//!   vibrato on the first sample is the tell.
//! - **Velocity goes somewhere** on every preset. A preset that ignores
//!   velocity is a preset for a sequencer, and this program's user plays.
//! - **Every preset names at least two macros**, and every named macro is read
//!   by at least one route — a knob that moves nothing is worth saying so.
//! - **Effects are a sound's, not a mix's**: a preset's reverb is the size of
//!   the instrument's own room, never the mix's hall.

use fontelle_dsp::{
    EnvelopeCurve, FilterModel, FilterRoute, FilterSlope, SvfMode, SynthSource, WarpMode,
    WavetableId,
};
use fontelle_types::{
    ChorusConfig, ChorusMode, DelayConfig, DistortionConfig, DistortionCurve, EffectConfig,
    LfoWave, NoteDivision, ReverbConfig,
};

use crate::mod_matrix::{Curve, ModDest, ModRoute, ModSource};
use crate::patch::{LfoMode, Patch, PatchFx, SILENT_DB, Source};

use super::flopsynth_init;

/// The categories the bank is organised into, in the order the browser and the
/// preset bar's drop-down show them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlopsynthCategory {
    Bass,
    Lead,
    Pad,
    Keys,
    Pluck,
    Strings,
    BrassAndWinds,
    ChoirAndVocal,
    Organ,
    BellsAndMallets,
    ChipAndRetro,
    SequenceAndArp,
    AtmosAndFx,
    SynthDrums,
    // ---- the electronic expansion (2026-09-09) ----
    //
    // Four shelves on a different axis from the fourteen above. Those answer
    // "what does a trombone sound like"; these answer "what can this synth do
    // that a sampler cannot", which is the question somebody opening a
    // *synthesiser* is actually asking. Kept apart rather than folded in for
    // two reasons: a person looking for the showpieces can find them, and
    // `every_pair_in_a_category_is_audibly_apart` is a claim *within* a
    // shelf — eighteen pads that must all differ from one another is already
    // near the limit of what a pad can be, and adding eight more to the pile
    // made thirty-five pairs collide at once.
    SyncAndFm,
    MotionAndMorph,
    BassMusic,
    Expressive,
    // ---- the second expansion (2026-09-09) ----
    //
    // The first four shelves were about *technique*. These four are about
    // **use**: the four jobs a general bank keeps being asked for and cannot
    // do — an instrument from outside the orchestra, a cue, something that
    // sounds like it came off tape, and a patch that plays itself.
    World,
    Cinematic,
    LoFiAndTape,
    Modular,
}

impl FlopsynthCategory {
    pub const ALL: [Self; 22] = [
        Self::Bass,
        Self::Lead,
        Self::Pad,
        Self::Keys,
        Self::Pluck,
        Self::Strings,
        Self::BrassAndWinds,
        Self::ChoirAndVocal,
        Self::Organ,
        Self::BellsAndMallets,
        Self::ChipAndRetro,
        Self::SequenceAndArp,
        Self::AtmosAndFx,
        Self::SynthDrums,
        Self::SyncAndFm,
        Self::MotionAndMorph,
        Self::BassMusic,
        Self::Expressive,
        Self::World,
        Self::Cinematic,
        Self::LoFiAndTape,
        Self::Modular,
    ];

    /// The folder name and the heading.
    ///
    /// A category **is a folder** (§P.3), so this string is on disk and is
    /// INVARIANT 7's: renaming one orphans everybody's presets.
    pub fn label(self) -> &'static str {
        match self {
            Self::Bass => "Bass",
            Self::Lead => "Lead",
            Self::Pad => "Pad",
            Self::Keys => "Keys",
            Self::Pluck => "Pluck",
            Self::Strings => "Strings",
            Self::BrassAndWinds => "Brass & Winds",
            Self::ChoirAndVocal => "Choir & Vocal",
            Self::Organ => "Organ",
            Self::BellsAndMallets => "Bells & Mallets",
            Self::ChipAndRetro => "Chip & Retro",
            Self::SequenceAndArp => "Sequence & Arp",
            Self::AtmosAndFx => "Atmos & FX",
            Self::SynthDrums => "Synth Drums",
            Self::SyncAndFm => "Sync & FM",
            Self::MotionAndMorph => "Motion & Morph",
            Self::BassMusic => "Bass Music",
            Self::Expressive => "Expressive",
            Self::World => "World",
            Self::Cinematic => "Cinematic",
            Self::LoFiAndTape => "Lo-Fi & Tape",
            Self::Modular => "Modular",
        }
    }
}

/// One row of the bank.
pub struct FactoryPreset {
    pub category: FlopsynthCategory,
    pub name: &'static str,
    pub build: fn() -> Patch,
}

// ---------------------------------------------------------- the builder ---
//
// Every row below reads as a sentence, which is the whole point: a preset is a
// design decision and a wall of struct literals is not reviewable. The builder
// is deliberately thin — every method writes exactly one field, so what a row
// says is what the patch is.

/// Layer indices, by role.
const A: usize = 0;
const B: usize = 1;
const C: usize = 2;
const SUB: usize = 3;
const NOISE: usize = 4;

struct Build {
    patch: Patch,
}

fn init() -> Build {
    Build {
        patch: flopsynth_init(),
    }
}

impl Build {
    fn osc_mut(&mut self, layer: usize) -> &mut fontelle_dsp::SynthOsc {
        match &mut self.patch.layers[layer].source {
            Source::Synth(osc) => osc,
            _ => unreachable!("a Flopsynth patch's first five layers are synth layers"),
        }
    }

    /// The table this layer reads, and the level it comes in at.
    fn osc(mut self, layer: usize, table: WavetableId, gain_db: f32) -> Self {
        self.osc_mut(layer).source = SynthSource::Table(table);
        self.patch.layers[layer].gain_db = gain_db;
        self
    }

    fn off(mut self, layer: usize) -> Self {
        self.patch.layers[layer].gain_db = SILENT_DB;
        self
    }

    fn pos(mut self, layer: usize, position: f32) -> Self {
        self.osc_mut(layer).position = position;
        self
    }

    /// Voices and the outermost one's detune, in cents.
    fn uni(mut self, layer: usize, voices: u8, detune_cents: f32) -> Self {
        let osc = self.osc_mut(layer);
        osc.unison.voices = voices;
        osc.unison.detune_cents = detune_cents;
        // A stack of one phase is a click that size; anything wider scatters.
        osc.random_phase = voices > 1;
        self
    }

    /// Unison voices that start **in step** rather than scattered.
    ///
    /// `uni` scatters the phases because a stack of one phase is a click
    /// that size — true of a saw, whose first sample is not zero. A table
    /// that starts at a zero crossing has no click to scatter, and three
    /// strings under one hammer start together and *then* drift apart: that
    /// drift is the piano's chorus, and scattered phases replace it with a
    /// different random comb on every note.
    fn locked(mut self, layer: usize) -> Self {
        self.osc_mut(layer).random_phase = false;
        self
    }

    /// How loud the unison's side voices are against its centre. Three
    /// equal voices drifting apart cancel to nothing twice a beat; a centre
    /// with quieter sides beside it dips instead, which is what a piano's
    /// unison does.
    fn blend(mut self, layer: usize, blend: f32) -> Self {
        self.osc_mut(layer).unison.blend = blend;
        self
    }

    fn width(mut self, layer: usize, width: f32) -> Self {
        self.osc_mut(layer).unison.width = width;
        self
    }

    fn semis(mut self, layer: usize, semitones: i8) -> Self {
        self.osc_mut(layer).semitones = semitones;
        self
    }

    fn fine(mut self, layer: usize, cents: f32) -> Self {
        self.patch.layers[layer].fine_tune_cents = cents;
        self
    }

    fn filter_route(mut self, layer: usize, route: FilterRoute) -> Self {
        self.osc_mut(layer).filter_route = route;
        self
    }

    fn warp(mut self, layer: usize, mode: WarpMode, amount: f32) -> Self {
        let osc = self.osc_mut(layer);
        osc.warp = mode;
        osc.warp_amount = amount;
        self
    }

    /// Which **later** layer feeds this one's FM or RM.
    fn modulator(mut self, layer: usize, from: usize) -> Self {
        debug_assert!(from > layer, "a modulator is always a later layer");
        self.osc_mut(layer).modulator = Some(from as u8);
        self
    }

    fn noise(mut self, colour: f32, gain_db: f32) -> Self {
        let osc = self.osc_mut(NOISE);
        osc.noise_colour = colour;
        self.patch.layers[NOISE].gain_db = gain_db;
        self
    }

    fn filter(
        mut self,
        slot: usize,
        model: FilterModel,
        mode: SvfMode,
        cutoff_hz: f32,
        resonance: f32,
    ) -> Self {
        let filter = &mut self.patch.filters[slot];
        filter.model = model;
        filter.mode = mode;
        filter.cutoff_hz = cutoff_hz;
        filter.resonance = resonance;
        filter.enabled = true;
        self
    }

    fn no_filter(mut self) -> Self {
        self.patch.filters[0].enabled = false;
        self.patch.filters[1].enabled = false;
        self
    }

    fn slope(mut self, slot: usize, slope: FilterSlope) -> Self {
        self.patch.filters[slot].slope = slope;
        self
    }

    fn character(mut self, slot: usize, character: f32) -> Self {
        self.patch.filters[slot].character = character;
        self
    }

    fn drive(mut self, slot: usize, drive: f32) -> Self {
        self.patch.filters[slot].drive = drive;
        self
    }

    fn key_track(mut self, slot: usize, amount: f32) -> Self {
        self.patch.filters[slot].key_track = amount;
        self
    }

    /// The amp envelope. Times in seconds.
    fn amp(mut self, attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        let env = &mut self.patch.envelopes[0];
        env.attack_s = attack;
        env.decay_s = decay;
        env.sustain_level = sustain;
        env.release_s = release;
        self
    }

    /// How long an envelope sits at the top before its decay starts.
    ///
    /// What a struck string does: a sampled grand is nearly level for its
    /// first two hundred milliseconds at middle C and only then falls, and a
    /// decay that starts the instant the hammer leaves is a pluck.
    fn hold(mut self, index: usize, seconds: f32) -> Self {
        self.patch.envelopes[index].hold_s = seconds;
        self
    }

    /// One of the three modulation envelopes — index 1, 2 or 3.
    fn env(mut self, index: usize, attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        let env = &mut self.patch.envelopes[index];
        env.attack_s = attack;
        env.decay_s = decay;
        env.sustain_level = sustain;
        env.release_s = release;
        self
    }

    /// An envelope's curve and the bend on its decay.
    ///
    /// The Init envelopes are linear in amplitude with a bent progress, which
    /// is a synth's shape. A struck string decays in *decibels* — a straight
    /// line on a level meter — so an imitation of one asks for
    /// [`EnvelopeCurve::Decibel`], where a stage time is the time to fall a
    /// hundred decibels. A negative bend on top of that is the two-slope
    /// decay a piano actually has: prompt sound fast, aftersound slow.
    fn curve(mut self, index: usize, curve: EnvelopeCurve, decay_shape: f32) -> Self {
        let env = &mut self.patch.envelopes[index];
        env.curve = curve;
        env.decay_shape = decay_shape;
        self
    }

    fn lfo(mut self, index: usize, wave: LfoWave, rate_hz: f32) -> Self {
        let lfo = &mut self.patch.lfos[index];
        lfo.wave = wave;
        lfo.rate_hz = rate_hz;
        lfo.sync = false;
        self
    }

    fn lfo_sync(mut self, index: usize, wave: LfoWave, division: NoteDivision) -> Self {
        let lfo = &mut self.patch.lfos[index];
        lfo.wave = wave;
        lfo.sync = true;
        lfo.division = division;
        self
    }

    /// The vibrato shape §7.3 asks of every acoustic imitation: late, and
    /// faded in.
    fn late(mut self, index: usize, delay_s: f32, fade_s: f32) -> Self {
        let lfo = &mut self.patch.lfos[index];
        lfo.delay_s = delay_s;
        lfo.fade_s = fade_s;
        self
    }

    fn lfo_mode(mut self, index: usize, mode: LfoMode) -> Self {
        self.patch.lfos[index].mode = mode;
        self
    }

    fn lfo_depth(mut self, index: usize, depth: f32) -> Self {
        self.patch.lfos[index].depth = depth;
        self
    }

    fn smooth(mut self, index: usize, smooth: f32) -> Self {
        self.patch.lfos[index].smooth = smooth;
        self
    }

    fn route(mut self, source: ModSource, destination: ModDest, depth: f32) -> Self {
        self.patch.mod_matrix.routes.push(ModRoute {
            source,
            destination,
            depth,
            curve: Curve::Linear,
            via: None,
            invert: false,
        });
        self
    }

    /// A route read as `1 − source`: full strength with the source at rest,
    /// falling away as it rises. What a key-tracked decay wants, because the
    /// knob tops out at ten seconds and a bass string rings past that: the
    /// *short* time is the one stored, and the route stretches the bottom of
    /// the keyboard rather than shrinking the top.
    fn inverted(mut self, source: ModSource, destination: ModDest, depth: f32) -> Self {
        self.patch.mod_matrix.routes.push(ModRoute {
            source,
            destination,
            depth,
            curve: Curve::Linear,
            via: None,
            invert: true,
        });
        self
    }

    fn route_via(
        mut self,
        source: ModSource,
        destination: ModDest,
        depth: f32,
        via: ModSource,
    ) -> Self {
        self.patch.mod_matrix.routes.push(ModRoute {
            source,
            destination,
            depth,
            curve: Curve::Linear,
            via: Some(via),
            invert: false,
        });
        self
    }

    /// A route snapped to whole steps — how a continuous source becomes an
    /// interval rather than a slide.
    fn stepped(mut self, source: ModSource, destination: ModDest, depth: f32, steps: u8) -> Self {
        self.patch.mod_matrix.routes.push(ModRoute {
            source,
            destination,
            depth,
            curve: Curve::Quantised { steps },
            via: None,
            invert: false,
        });
        self
    }

    /// The filter envelope's depth — the route the Init patch already wrote,
    /// turned up rather than added again.
    fn env_to_cut(mut self, depth: f32) -> Self {
        if let Some(route) = self.patch.mod_matrix.routes.iter_mut().find(|r| {
            r.destination == ModDest::FilterCutoff(0) && r.source == ModSource::Envelope(1)
        }) {
            route.depth = depth;
        }
        self
    }

    /// Names a macro. Every named macro must be read by a route — see the
    /// module docs, and `tests/flopsynth_presets.rs`, which enforces it.
    fn mac(mut self, index: usize, name: &str) -> Self {
        self.patch.macros[index].name = name.to_string();
        self
    }

    fn fx(mut self, config: EffectConfig) -> Self {
        self.patch.fx.push(PatchFx {
            config,
            enabled: true,
        });
        self
    }

    fn mono(mut self, glide_s: f32) -> Self {
        self.patch.voice_config.retrigger = crate::voice::RetriggerMode::Legato;
        self.patch.voice_config.glide_time_s = glide_s;
        self.patch.voice_config.glide_legato_only = true;
        self
    }

    /// The loudness trim §7.4 matches the bank on.
    ///
    /// One number per preset, produced by the measuring pass
    /// (`examples/preset_probe.rs`) rather than by ear — §13's third risk is
    /// that tuning this by hand is slow, and the answer is to measure the
    /// whole bank at once and write the column.
    ///
    /// It goes into [`Patch::output_db`] where that can hold it and into the
    /// layers' own levels where it cannot. The trim knob runs −24..+12 dB, and
    /// three of the categories need more than that in one direction or the
    /// other: a formant filter is a narrow window on a spectrum, so a choir
    /// arrives twenty decibels below a saw, and a gated sequence arrives
    /// thirty above. Spilling the remainder into the layers keeps the *knob*
    /// meaningful — somebody who opens one of these and turns the output down
    /// still has the range they expect either side of where it sits.
    ///
    /// **Called last in every row**, because it reads the levels the row has
    /// already set.
    fn out(mut self, db: f32) -> Self {
        let trim = db.clamp(
            crate::patch_params::OUTPUT_MIN_DB,
            crate::patch_params::OUTPUT_MAX_DB,
        );
        self.patch.output_db = trim;
        let rest = db - trim;
        if rest.abs() > f32::EPSILON {
            for layer in &mut self.patch.layers {
                // A layer that is off stays off: the trim is about how loud
                // the preset is, not about which of its oscillators are in it.
                if layer.gain_db > SILENT_DB {
                    layer.gain_db = (layer.gain_db + rest)
                        .clamp(SILENT_DB + 0.1, crate::patch_params::GAIN_MAX_DB);
                }
            }
        }
        self
    }

    fn done(self) -> Patch {
        self.patch
    }
}

// ------------------------------------------------------- effect helpers ---

fn chorus(voices: u32, mix: f32) -> EffectConfig {
    EffectConfig::Chorus(ChorusConfig {
        voices,
        mix,
        ..ChorusConfig::new()
    })
}

/// The string machine's chorus: every voice on its own LFO, so they never come
/// back into step. What an "ensemble" was.
fn ensemble(voices: u32, mix: f32) -> EffectConfig {
    EffectConfig::Chorus(ChorusConfig {
        voices,
        mode: ChorusMode::Ensemble,
        rate_hz: 0.35,
        depth: 0.7,
        spread: 0.9,
        mix,
        ..ChorusConfig::new()
    })
}

fn reverb(size: f32, mix: f32) -> EffectConfig {
    EffectConfig::Reverb(ReverbConfig {
        size,
        decay_s: 1.0 + size * 5.0,
        mix,
        ..ReverbConfig::new()
    })
}

fn delay(division: NoteDivision, feedback: f32, mix: f32) -> EffectConfig {
    EffectConfig::Delay(DelayConfig {
        sync: true,
        division,
        feedback,
        mix,
        ..DelayConfig::new()
    })
}

fn ping_pong(division: NoteDivision, feedback: f32, mix: f32) -> EffectConfig {
    EffectConfig::Delay(DelayConfig {
        sync: true,
        division,
        feedback,
        ping_pong: true,
        mix,
        ..DelayConfig::new()
    })
}

fn drive_fx(curve: DistortionCurve, drive_db: f32, mix: f32) -> EffectConfig {
    EffectConfig::Distortion(DistortionConfig {
        curve,
        drive_db,
        mix,
        ..DistortionConfig::new()
    })
}

fn crush(bits: f32, rate_hz: f32, mix: f32) -> EffectConfig {
    EffectConfig::Bitcrush(fontelle_types::BitcrushConfig {
        bits,
        rate_hz,
        mix,
        ..fontelle_types::BitcrushConfig::new()
    })
}

// ------------------------------------------------------------ archetypes ---
//
// The dozen shapes the two hundred and ten rows are variations of. Each is the
// answer to "what *is* this kind of sound", written once.

/// Two saw stacks an octave apart, a low-pass that opens with velocity, a
/// vibrato that arrives late, and the ensemble chorus — which is what a string
/// machine was.
fn strings(cutoff: f32, attack: f32, release: f32) -> Build {
    init()
        .osc(A, WavetableId::Saw, -16.0)
        .uni(A, 5, 12.0)
        .width(A, 0.8)
        .osc(B, WavetableId::Saw, -22.0)
        .semis(B, -12)
        .uni(B, 3, 9.0)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, cutoff, 0.5)
        .slope(0, FilterSlope::Db24)
        .amp(attack, 0.0, 1.0, release)
        .lfo(0, LfoWave::Sine, 5.2)
        .late(0, 0.35, 0.4)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.000_6)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(B as u8), 0.000_6)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.12)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.15)
        .route_via(
            ModSource::Lfo(0),
            ModDest::LayerPitch(A as u8),
            0.001_2,
            ModSource::ModWheel,
        )
        .route(ModSource::Macro(1), ModDest::LfoDepth(0), 0.6)
        .mac(0, "Brightness")
        .mac(1, "Vibrato")
}

/// A detuned stack (or the Choir table) into the **formant** filter, a slow
/// attack, breath noise at a whisper, a wide ensemble and a hall. The "choir
/// ahhs" the brief names.
fn choir(vowel: f32, attack: f32) -> Build {
    init()
        .osc(A, WavetableId::Choir, -15.0)
        .pos(A, 0.4)
        .uni(A, 5, 10.0)
        .width(A, 0.9)
        .osc(B, WavetableId::Saw, -24.0)
        .semis(B, -12)
        .uni(B, 3, 7.0)
        .noise(0.8, -40.0)
        .filter_route(NOISE, FilterRoute::F2)
        // A high Q, because the vowel has to be **decisive**: at a gentle
        // one the Choir table's own spectrum is what you hear and every
        // vowel setting is the same sound.
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 1_000.0, 0.7)
        .character(0, vowel)
        .filter(1, FilterModel::Clean, SvfMode::Bandpass, 3_000.0, 0.5)
        .amp(attack, 0.0, 1.0, 1.2)
        .lfo(0, LfoWave::Sine, 4.5)
        .late(0, 0.5, 0.6)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.000_5)
        .route(ModSource::Velocity, ModDest::FilterCharacter(0), 0.06)
        .route(ModSource::Macro(0), ModDest::FilterCharacter(0), 0.5)
        .route_via(
            ModSource::Lfo(0),
            ModDest::LayerPitch(A as u8),
            0.001,
            ModSource::ModWheel,
        )
        .mac(0, "Vowel")
        .mac(1, "Air")
        .route(ModSource::Macro(1), ModDest::LayerGain(NOISE as u8), 0.15)
        .fx(ensemble(4, 0.4))
        .fx(reverb(0.8, 0.4))
}

/// A saw through a **ladder** whose envelope attacks fast and overshoots to a
/// lower sustain, with a few cents of pitch envelope on the attack. Brass.
fn brass(cutoff: f32, attack: f32) -> Build {
    init()
        .osc(A, WavetableId::Saw, -15.0)
        .uni(A, 3, 9.0)
        .osc(B, WavetableId::Saw, -23.0)
        .semis(B, -12)
        .uni(B, 2, 6.0)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, cutoff, 0.35)
        .character(0, 0.35)
        .amp(attack, 0.0, 1.0, 0.25)
        .env(1, 0.04, 0.25, 0.55, 0.25)
        .env_to_cut(0.55)
        .env(2, 0.0, 0.06, 0.0, 0.05)
        .route(ModSource::Envelope(2), ModDest::LayerPitch(A as u8), -0.003)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.2)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.25)
        .route(ModSource::Macro(1), ModDest::FilterCharacter(0), 0.5)
        .mac(0, "Brightness")
        .mac(1, "Drive")
}

/// Two-operator FM at a bell ratio, with a decay that outlives the note and a
/// bright strike that does not.
fn bell(table: WavetableId, decay: f32) -> Build {
    init()
        .osc(A, table, -13.0)
        .pos(A, 0.5)
        .off(B)
        .off(C)
        .off(SUB)
        .no_filter()
        .amp(0.002, decay, 0.0, decay * 0.6)
        .env(2, 0.0, 0.3, 0.0, 0.2)
        .route(ModSource::Envelope(2), ModDest::OscPosition(A as u8), 0.35)
        .route(ModSource::Velocity, ModDest::OscPosition(A as u8), 0.25)
        .route(ModSource::Macro(0), ModDest::OscPosition(A as u8), 0.4)
        // The shimmer, off at rest: a bell that wobbled by default would be a
        // bell nobody could use straight.
        .lfo(0, LfoWave::Sine, 0.25)
        .lfo_depth(0, 0.0)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.3)
        .route(ModSource::Macro(1), ModDest::LfoDepth(0), 1.0)
        .mac(0, "Strike")
        .mac(1, "Shimmer")
}

/// The Yamaha e-piano: a sine carrier, a sine modulator fourteen semitones and
/// two octaves above it, and the bell in the attack that a short envelope on
/// the FM index makes.
fn electric_piano(index: f32, decay: f32) -> Build {
    init()
        .osc(A, WavetableId::Sine, -11.0)
        .warp(A, WarpMode::Fm, index)
        .modulator(A, B)
        // The modulator itself is never heard: its level is what it
        // contributes to the mix, not whether it modulates.
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 43)
        .off(C)
        .off(SUB)
        .no_filter()
        .amp(0.002, decay, 0.28, 0.35)
        .env(2, 0.0, 0.4, 0.0, 0.2)
        .route(ModSource::Envelope(2), ModDest::OscWarp(A as u8), 0.35)
        .route(ModSource::Velocity, ModDest::OscWarp(A as u8), 0.3)
        .route(ModSource::Macro(0), ModDest::OscWarp(A as u8), 0.4)
        .mac(0, "Bite")
        // The motor. The LFO starts at rest and the macro is what brings it
        // in, which is what makes it one knob rather than two.
        .mac(1, "Tremolo")
        .lfo(1, LfoWave::Sine, 5.5)
        .lfo_depth(1, 0.0)
        .route(ModSource::Lfo(1), ModDest::Amp, 0.35)
        .route(ModSource::Macro(1), ModDest::LfoDepth(1), 1.0)
}

/// A stack, a filter that shuts almost at once, and no sustain. What every
/// plucked sound is.
fn pluck(table: WavetableId, cutoff: f32, decay: f32) -> Build {
    init()
        .osc(A, table, -13.0)
        .uni(A, 2, 8.0)
        .off(B)
        .off(C)
        .off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, cutoff, 0.5)
        .slope(0, FilterSlope::Db24)
        .amp(0.001, decay, 0.0, decay * 0.35)
        .env(1, 0.0, 0.2, 0.0, 0.15)
        .env_to_cut(0.6)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.25)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::FilterResonance(0), 0.5)
        .mac(0, "Tone")
        .mac(1, "Bite")
}

/// A big detuned stack with a slow attack and a long release, moving under a
/// slow LFO. Every pad.
fn pad(table: WavetableId, cutoff: f32, attack: f32, release: f32) -> Build {
    init()
        .osc(A, table, -17.0)
        .uni(A, 5, 14.0)
        .width(A, 0.9)
        .osc(B, WavetableId::Saw, -24.0)
        .semis(B, -12)
        .uni(B, 3, 10.0)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, cutoff, 0.4)
        .slope(0, FilterSlope::Db24)
        .amp(attack, 0.0, 1.0, release)
        .lfo(0, LfoWave::Sine, 0.2)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.15)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.15)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::LfoRate(0), 0.4)
        .route_via(
            ModSource::Lfo(0),
            ModDest::FilterCutoff(0),
            0.3,
            ModSource::ModWheel,
        )
        .mac(0, "Brightness")
        .mac(1, "Motion")
        .fx(chorus(3, 0.35))
        .fx(reverb(0.7, 0.35))
}

/// One oscillator, mono and legato, with a vibrato on the wheel. Every lead.
fn lead(table: WavetableId, cutoff: f32, glide: f32) -> Build {
    init()
        .osc(A, table, -13.0)
        .uni(A, 3, 10.0)
        .off(B)
        .off(C)
        .off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, cutoff, 0.4)
        .slope(0, FilterSlope::Db24)
        .amp(0.005, 0.0, 1.0, 0.2)
        .mono(glide)
        .lfo(0, LfoWave::Sine, 5.5)
        .late(0, 0.25, 0.35)
        .route_via(
            ModSource::Lfo(0),
            ModDest::LayerPitch(A as u8),
            0.001_5,
            ModSource::ModWheel,
        )
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.2)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::OscUnisonDetune(A as u8), 0.5)
        .mac(0, "Brightness")
        .mac(1, "Detune")
}

/// A sub, a body, and a filter low enough that the top of the sound is the
/// envelope rather than the harmonics. Every bass.
fn bass(table: WavetableId, cutoff: f32, release: f32) -> Build {
    init()
        .osc(A, table, -13.0)
        .off(B)
        .off(C)
        .osc(SUB, WavetableId::SubSine, -17.0)
        .semis(SUB, -12)
        .filter_route(SUB, FilterRoute::Bypass)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, cutoff, 0.2)
        .slope(0, FilterSlope::Db24)
        .amp(0.003, 0.0, 1.0, release)
        .env(1, 0.0, 0.25, 0.0, 0.15)
        .env_to_cut(0.28)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.25)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.35)
        .route(ModSource::Macro(1), ModDest::FilterResonance(0), 0.6)
        .mac(0, "Cutoff")
        .mac(1, "Resonance")
}

/// A drawbar organ: no filter, an instant envelope, a click, and a Leslie.
fn organ(position: f32, leslie_hz: f32) -> Build {
    init()
        .osc(A, WavetableId::Drawbar, -12.0)
        .pos(A, position)
        .off(B)
        .off(C)
        .off(SUB)
        .no_filter()
        .amp(0.004, 0.0, 1.0, 0.04)
        // The key click: a burst of noise through a high-pass, gated by its
        // own envelope, which is what a Hammond's contacts actually are.
        .noise(0.0, -30.0)
        .filter_route(NOISE, FilterRoute::F2)
        .filter(1, FilterModel::Clean, SvfMode::Highpass, 2_000.0, 0.7)
        .env(2, 0.0, 0.008, 0.0, 0.008)
        .route(ModSource::Envelope(2), ModDest::LayerGain(NOISE as u8), 0.2)
        .lfo(0, LfoWave::Sine, leslie_hz)
        .route(ModSource::Lfo(0), ModDest::LayerPan(A as u8), 0.5)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.000_3)
        .route(ModSource::Velocity, ModDest::LayerGain(NOISE as u8), 0.15)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.5)
        .route(ModSource::Macro(1), ModDest::OscPosition(A as u8), 0.5)
        .mac(0, "Leslie")
        .mac(1, "Drawbars")
}

/// One chip channel, exactly: no filter, a hard envelope, and the arcade trill
/// on the wheel.
fn chip(table: WavetableId, release: f32) -> Build {
    init()
        .osc(A, table, -14.0)
        .off(B)
        .off(C)
        .off(SUB)
        .no_filter()
        .amp(0.0, 0.0, 1.0, release)
        .lfo(0, LfoWave::Square, 7.0)
        .route_via(
            ModSource::Lfo(0),
            ModDest::LayerPitch(A as u8),
            0.003,
            ModSource::ModWheel,
        )
        .route(ModSource::Velocity, ModDest::LayerGain(A as u8), 0.06)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.5)
        .route(ModSource::Macro(1), ModDest::LfoDepth(0), 0.6)
        .mac(0, "Trill rate")
        .mac(1, "Trill depth")
}

/// Noise, a band-pass on a slow LFO, and a long everything. Weather.
fn atmos(colour: f32, mode: SvfMode, cutoff: f32) -> Build {
    init()
        .off(A)
        .off(B)
        .off(C)
        .off(SUB)
        .noise(colour, -8.0)
        .filter_route(NOISE, FilterRoute::F1)
        .filter(0, FilterModel::Clean, mode, cutoff, 0.5)
        .amp(1.5, 0.0, 1.0, 2.5)
        .lfo(0, LfoWave::Sine, 0.08)
        .lfo_mode(0, LfoMode::Free)
        .lfo(1, LfoWave::Triangle, 0.13)
        .lfo_mode(1, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.6)
        .route(ModSource::Lfo(1), ModDest::FilterResonance(0), 0.3)
        .route(ModSource::Velocity, ModDest::LayerGain(NOISE as u8), 0.1)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.4)
        .route(ModSource::Macro(1), ModDest::LfoRate(0), 0.6)
        .mac(0, "Tone")
        .mac(1, "Speed")
        .fx(reverb(0.9, 0.45))
}

/// A pitch envelope steep enough to be a transient, and a body that ends
/// itself. Every synthesised drum.
fn drum(table: WavetableId, drop_semitones: f32, drop_s: f32, decay: f32) -> Build {
    init()
        .osc(A, table, -8.0)
        .off(B)
        .off(C)
        .off(SUB)
        .no_filter()
        .amp(0.002, decay, 0.0, decay * 0.5)
        .env(1, 0.0, drop_s, 0.0, drop_s)
        .route(
            ModSource::Envelope(1),
            ModDest::LayerPitch(A as u8),
            drop_semitones * 100.0 / 9_600.0,
        )
        .route(ModSource::Velocity, ModDest::LayerGain(A as u8), 0.12)
        .route(ModSource::Macro(0), ModDest::LayerPitch(A as u8), 0.02)
        // The transient. Off at rest, so a kick is a kick until somebody
        // wants a click on it.
        .noise(0.0, SILENT_DB)
        .route(ModSource::Macro(1), ModDest::LayerGain(NOISE as u8), 0.45)
        .mac(0, "Tune")
        .mac(1, "Click")
}

// -------------------------------------------------------------- the bank ---
//
// One row per preset, in the order the browser lists them. A row is a
// sentence: the archetype it is, and what is different about it.

macro_rules! bank {
    ($($category:ident : $name:literal => $build:expr,)*) => {
        /// Every factory preset, in the order the browser lists them.
        ///
        /// The count and the categories are held by
        /// `tests/flopsynth_presets.rs`, not by this table — so adding a row is
        /// adding a row, and the gate that it still sounds, still fits inside
        /// full scale, still sits within three decibels of its neighbours and
        /// is still audibly apart from every other preset in its category is
        /// the test's to enforce.
        pub static FACTORY: &[FactoryPreset] = &[
            $(FactoryPreset {
                category: FlopsynthCategory::$category,
                name: $name,
                build: || { let b: Build = $build; b.done() },
            },)*
        ];
    };
}

bank! {
    // ---------------------------------------------------------------- Bass ---
    // Every one has the sub on and its trim set so switching between them does
    // not jump — except the three whose filter is a narrow window, where the
    // sub goes around the filter (see `bass`) and would be most of what came
    // out. There, the body is the sound and the sub is what hides it.
    // The plain one: a saw and a sub, a filter envelope you can hear, and an
    // amp that settles back rather than holding — against Moog Stack's three
    // oscillators under a filter that stays where it is put.
    Bass: "Init Bass" => bass(WavetableId::Saw, 1_200.0, 0.12)
        .amp(0.003, 0.55, 0.5, 0.12)
        .env_to_cut(0.5).out(1.7),
    Bass: "Sub Sine" => bass(WavetableId::SubSine, 20_000.0, 0.15)
        .no_filter()
        .noise(0.0, -34.0)
        .env(2, 0.0, 0.01, 0.0, 0.01)
        .route(ModSource::Envelope(2), ModDest::LayerGain(NOISE as u8), 0.25)
        .out(-4.1),
    Bass: "Reese" => bass(WavetableId::Reese, 520.0, 0.5)
        .pos(A, 0.4)
        .uni(A, 2, 12.0)
        .osc(B, WavetableId::Saw, -19.0)
        .semis(B, -12)
        .uni(B, 2, 8.0)
        .amp(0.06, 0.0, 1.0, 0.5)
        .env_to_cut(0.1)
        .fx(chorus(2, 0.3))
        .out(0.6),
    // The squelch: a ladder driven into its own saturation before the filter,
    // which is the one thing a clean filter cannot do and the reason the
    // model has a drive knob at all.
    Bass: "Acid" => bass(WavetableId::Saw, 420.0, 0.04)
        .drive(0, 0.45)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 600.0, 0.62)
        .character(0, 0.6)
        .env(1, 0.0, 0.12, 0.0, 0.08)
        .env_to_cut(0.62)
        .mono(0.06)
        .fx(drive_fx(DistortionCurve::SoftClip, 14.0, 0.4))
        .out(-0.2),
    // Three oscillators an octave apart and all of them in tune, which is what
    // a Moog stack *is* — against Init Bass's one saw and a sub. The filter
    // stays where it is put: the stack is the sound, not the sweep.
    Bass: "Moog Stack" => bass(WavetableId::Square, 5_000.0, 0.06)
        .osc(B, WavetableId::Saw, -17.0)
        .semis(B, -12)
        .osc(C, WavetableId::Triangle, -19.0)
        .semis(C, -24)
        .fine(C, 7.0)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 5_000.0, 0.7)
        .env_to_cut(0.0)
        .mono(0.03)
        .out(5.0),
    Bass: "FM Growl" => bass(WavetableId::Sine, 3_000.0, 0.1)
        .warp(A, WarpMode::Fm, 0.6)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 19)
        .lfo_sync(0, LfoWave::Sine, NoteDivision::Eighth)
        .route(ModSource::Lfo(0), ModDest::OscWarp(A as u8), 0.3)
        .out(-1.0),
    Bass: "Pluck Bass" => bass(WavetableId::AnalogMorph, 400.0, 0.1)
        .pos(A, 0.7)
        .amp(0.002, 0.35, 0.0, 0.12)
        .env(1, 0.0, 0.12, 0.0, 0.1)
        .env_to_cut(0.8)
        .out(3.1),
    Bass: "Wobble" => bass(WavetableId::Growl, 180.0, 0.3)
        .pos(A, 0.2)
        .uni(A, 3, 14.0)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 300.0, 0.35)
        .amp(0.02, 0.0, 1.0, 0.3)
        .lfo_sync(0, LfoWave::Sine, NoteDivision::Quarter)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.35)
        .route(ModSource::Macro(1), ModDest::LfoRate(0), 0.7)
        .mac(1, "Wobble rate")
        .out(1.9),
    Bass: "Chip Bass" => bass(WavetableId::NesPulse25, 20_000.0, 0.02)
        .no_filter()
        .osc(SUB, WavetableId::NesTriangle, -17.0)
        .amp(0.0, 0.0, 1.0, 0.02)
        .fx(crush(12.0, 22_050.0, 0.3))
        .out(-4.7),
    Bass: "Distorted" => bass(WavetableId::Grit, 9_000.0, 0.04)
        .pos(A, 0.1)
        .uni(A, 3, 15.0)
        .filter(0, FilterModel::Clean, SvfMode::Highpass, 120.0, 0.7)
        .amp(0.001, 0.5, 0.35, 0.04)
        .env_to_cut(0.55)
        .fx(drive_fx(DistortionCurve::Diode, 18.0, 0.5))
        .out(2.7),
    Bass: "Warm Round" => bass(WavetableId::SubSine, 1_400.0, 0.6)
        .osc(B, WavetableId::SubTri, -15.0)
        .semis(B, 12)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 1_400.0, 0.6)
        .key_track(0, 0.9)
        .amp(0.05, 0.0, 1.0, 0.6)
        .env_to_cut(0.06)
        .out(-5.6),
    Bass: "Neuro" => bass(WavetableId::Growl, 2_000.0, 0.35)
        .pos(A, 0.5)
        .uni(A, 2, 10.0)
        .osc(B, WavetableId::Grit, -22.0)
        .semis(B, 12)
        .warp(A, WarpMode::Rm, 0.5)
        .modulator(A, B)
        .filter(0, FilterModel::Comb, SvfMode::Lowpass, 220.0, 0.3)
        .character(0, 0.7)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 2_000.0, 0.5)
        .lfo_sync(1, LfoWave::Triangle, NoteDivision::Sixteenth)
        .lfo_mode(1, LfoMode::Free)
        .route(ModSource::Lfo(1), ModDest::FilterCutoff(0), 0.5)
        .fx(drive_fx(DistortionCurve::SoftClip, 10.0, 0.35))
        .fx(chorus(2, 0.25))
        .out(2.1),

    // The acoustic ones. A comb filter tuned to the note is a string's *body*,
    // and it is the only thing that makes a bass sound like it has wood in it
    // — which is why these are not "Init Bass, darker".
    Bass: "Upright" => bass(WavetableId::Triangle, 900.0, 0.25)
        .semis(A, -12)
        .osc(B, WavetableId::Saw, -26.0)
        .semis(B, -12)
        .filter(0, FilterModel::Comb, SvfMode::Lowpass, 170.0, 0.4)
        .character(0, 0.55)
        .key_track(0, 1.0)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 1_600.0, 0.5)
        .noise(0.5, -28.0)
        .filter_route(NOISE, FilterRoute::F2)
        .amp(0.004, 1.1, 0.0, 0.2)
        .env(2, 0.0, 0.03, 0.0, 0.02)
        .route(ModSource::Envelope(2), ModDest::LayerGain(NOISE as u8), 0.4)
        .mono(0.05)
        .out(3.7),
    Bass: "Slap" => bass(WavetableId::Square, 700.0, 0.1)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 620.0, 0.78)
        .character(0, 0.5)
        .env(1, 0.0, 0.055, 0.0, 0.05)
        .env_to_cut(0.9)
        .amp(0.001, 0.5, 0.12, 0.1)
        .noise(0.0, -24.0)
        .filter_route(NOISE, FilterRoute::F2)
        .filter(1, FilterModel::Clean, SvfMode::Highpass, 4_000.0, 0.7)
        .env(2, 0.0, 0.012, 0.0, 0.01)
        .route(ModSource::Envelope(2), ModDest::LayerGain(NOISE as u8), 0.4)
        .fx(drive_fx(DistortionCurve::SoftClip, 8.0, 0.25))
        .out(3.8),
    Bass: "Rubber" => bass(WavetableId::Square, 500.0, 0.18)
        .warp(A, WarpMode::Mirror, 0.45)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 460.0, 0.55)
        .character(0, 0.25)
        .env(1, 0.0, 0.2, 0.0, 0.14)
        .env_to_cut(0.55)
        .amp(0.002, 0.6, 0.35, 0.15)
        .route(ModSource::Macro(2), ModDest::OscWarp(A as u8), 0.4)
        .mac(2, "Hollow")
        .fx(chorus(2, 0.25))
        .out(0.6),
    Bass: "Fretless" => bass(WavetableId::Triangle, 9_000.0, 0.35)
        .osc(SUB, WavetableId::SubSine, -25.0)
        .osc(B, WavetableId::Sine, -20.0)
        .semis(B, 19)
        // The low-pass first and the body **after** it, so the envelope's
        // bloom on the attack is a bloom and not a body that changes shape.
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 9_000.0, 0.4)
        .key_track(0, 0.6)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Comb, SvfMode::Lowpass, 300.0, 0.25)
        .character(1, 0.85)
        .key_track(1, 1.0)
        .amp(0.02, 0.0, 1.0, 0.8)
        .env(1, 0.0, 0.4, 0.0, 0.25)
        .env_to_cut(0.5)
        .mono(0.14)
        .lfo(0, LfoWave::Sine, 4.8)
        .late(0, 0.45, 0.5)
        .route_via(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.001_2, ModSource::ModWheel)
        .fx(reverb(0.4, 0.2))
        .out(7.3),
    // Ring modulation at a ratio that is not a harmonic: the sidebands land
    // *between* the partials, which is a bell's spectrum on a bass's envelope
    // and is nothing a ladder can be made to do.
    Bass: "Metallic" => bass(WavetableId::Saw, 6_000.0, 0.4)
        .uni(A, 1, 0.0)
        .warp(A, WarpMode::Rm, 1.0)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 11)
        .off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 6_000.0, 0.3)
        .amp(0.002, 1.4, 0.22, 0.4)
        .env(1, 0.0, 0.2, 0.0, 0.15)
        .env_to_cut(0.2)
        .route(ModSource::Macro(2), ModDest::OscWarp(A as u8), 0.5)
        .mac(2, "Metal")
        .out(10.9),
    Bass: "Bowed" => bass(WavetableId::Sawstack, 20_000.0, 0.6)
        .pos(A, 0.35)
        .uni(A, 2, 5.0)
        .off(SUB)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 900.0, 0.35)
        .character(0, 0.6)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 3_000.0, 0.5)
        .amp(0.3, 0.0, 1.0, 0.7)
        .env_to_cut(0.0)
        .lfo(0, LfoWave::Sine, 4.6)
        .late(0, 0.5, 0.5)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.000_5)
        .fx(ensemble(3, 0.3))
        .out(14.0),

    // ---------------------------------------------------------------- Lead ---
    Lead: "Init Lead" => lead(WavetableId::Saw, 4_000.0, 0.04)
        .uni(A, 3, 10.0)
        .amp(0.008, 0.0, 1.0, 0.12)
        .fx(delay(NoteDivision::Eighth, 0.35, 0.2))
        .out(6.0),
    Lead: "Supersaw" => lead(WavetableId::Saw, 9_000.0, 0.0)
        .amp(0.03, 0.0, 1.0, 0.7)
        .uni(A, 7, 22.0)
        .width(A, 0.9)
        .osc(B, WavetableId::Saw, -20.0)
        .semis(B, 12)
        .uni(B, 7, 18.0)
        .fx(chorus(3, 0.3))
        .fx(reverb(0.4, 0.2))
        .out(5.3),
    Lead: "Sync Lead" => lead(WavetableId::SyncSweep, 6_000.0, 0.02)
        .pos(A, 0.3)
        .warp(A, WarpMode::Sync, 0.5)
        .env(1, 0.0, 0.25, 0.0, 0.2)
        .route(ModSource::Envelope(1), ModDest::OscWarp(A as u8), 0.6)
        .out(8.2),
    Lead: "Square Lead" => lead(WavetableId::Pulse, 5_000.0, 0.0)
        .pos(A, 0.3)
        .uni(A, 1, 0.0)
        .lfo(1, LfoWave::Triangle, 0.3)
        .route(ModSource::Lfo(1), ModDest::OscPosition(A as u8), 0.2)
        .late(0, 0.3, 0.4)
        .out(16.7),
    Lead: "Hoover" => lead(WavetableId::Hoover, 3_000.0, 0.08)
        .pos(A, 0.5)
        .uni(A, 4, 30.0)
        .osc(B, WavetableId::Saw, -19.0)
        .semis(B, -12)
        .uni(B, 2, 12.0)
        .amp(0.05, 0.0, 1.0, 0.5)
        .fx(ensemble(3, 0.35))
        .fx(ping_pong(NoteDivision::Eighth, 0.3, 0.25))
        .out(11.2),
    Lead: "Soft Sine" => lead(WavetableId::Sine, 3_000.0, 0.05)
        .uni(A, 1, 0.0)
        .osc(B, WavetableId::Triangle, -25.0)
        .semis(B, -12)
        .amp(0.04, 0.0, 1.0, 0.4)
        .late(0, 0.25, 0.3)
        .fx(reverb(0.5, 0.3))
        .out(5.0),
    Lead: "Bright Pulse" => lead(WavetableId::NesPulse125, 20_000.0, 0.0)
        .uni(A, 1, 0.0)
        .no_filter()
        .amp(0.0, 0.0, 1.0, 0.03)
        .fx(delay(NoteDivision::EighthDotted, 0.35, 0.3))
        .out(4.8),
    Lead: "Screamer" => lead(WavetableId::Saw, 3_000.0, 0.02)
        .uni(A, 2, 12.0)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 3_000.0, 0.55)
        .character(0, 0.8)
        .fx(drive_fx(DistortionCurve::Tube, 20.0, 0.6))
        .fx(delay(NoteDivision::Eighth, 0.3, 0.2))
        .out(12.4),
    Lead: "Whistle" => lead(WavetableId::Sine, 4_000.0, 0.03)
        .uni(A, 1, 0.0)
        .semis(A, 12)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 4_000.0, 1.5)
        .noise(0.2, -34.0)
        .amp(0.06, 0.0, 1.0, 0.2)
        .lfo(0, LfoWave::Sine, 5.0)
        .out(30.6),
    Lead: "Talk Lead" => lead(WavetableId::Saw, 20_000.0, 0.03)
        .uni(A, 2, 8.0)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 1_000.0, 0.5)
        .character(0, 0.2)
        .lfo(1, LfoWave::Triangle, 0.4)
        .route(ModSource::Lfo(1), ModDest::FilterCharacter(0), 0.5)
        .route(ModSource::Macro(0), ModDest::FilterCharacter(0), 0.8)
        .mac(0, "Vowel")
        .out(11.6),
    Lead: "Portamento" => lead(WavetableId::Saw, 4_000.0, 0.18)
        .uni(A, 1, 0.0)
        .osc(B, WavetableId::Square, -21.0)
        .semis(B, -12)
        .amp(0.005, 0.0, 1.0, 0.3)
        .out(7.1),
    Lead: "Retro Lead" => lead(WavetableId::C64, 6_000.0, 0.0)
        .amp(0.0, 0.18, 0.55, 0.03)
        .pos(A, 0.5)
        .uni(A, 1, 0.0)
        .lfo(0, LfoWave::Square, 6.0)
        .route_via(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.004, ModSource::ModWheel)
        .fx(crush(8.0, 22_050.0, 0.2))
        .out(13.0),

    Lead: "Fifths" => lead(WavetableId::Saw, 5_000.0, 0.02)
        .uni(A, 2, 6.0)
        .osc(B, WavetableId::Saw, -17.0)
        .semis(B, 7)
        .osc(C, WavetableId::Saw, -23.0)
        .semis(C, 12)
        .amp(0.006, 0.0, 1.0, 0.15)
        .fx(chorus(2, 0.25))
        .out(4.7),
    Lead: "Bell Lead" => lead(WavetableId::Sine, 8_000.0, 0.03)
        .uni(A, 1, 0.0)
        .warp(A, WarpMode::Fm, 0.35)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 19)
        .amp(0.002, 1.2, 0.25, 0.3)
        .env(2, 0.0, 0.35, 0.0, 0.2)
        .route(ModSource::Envelope(2), ModDest::OscWarp(A as u8), 0.4)
        .route(ModSource::Macro(2), ModDest::OscWarp(A as u8), 0.4)
        .mac(2, "Bite")
        .fx(delay(NoteDivision::EighthDotted, 0.3, 0.2))
        .out(6.7),
    Lead: "Ring Lead" => lead(WavetableId::Square, 6_000.0, 0.02)
        .uni(A, 1, 0.0)
        .warp(A, WarpMode::Rm, 0.8)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 14)
        .amp(0.004, 0.0, 1.0, 0.12)
        .lfo(1, LfoWave::Triangle, 0.25)
        .route(ModSource::Lfo(1), ModDest::OscWarp(A as u8), 0.3)
        .route(ModSource::Macro(2), ModDest::LfoRate(1), 0.5)
        .mac(2, "Drift")
        .fx(ping_pong(NoteDivision::Eighth, 0.35, 0.25))
        .out(9.6),
    // The ladder at the edge of self-oscillation, swept by its own envelope:
    // the resonance is the note's top voice rather than a colour on it.
    Lead: "Reso Sweep" => lead(WavetableId::Saw, 900.0, 0.05)
        .uni(A, 2, 7.0)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 700.0, 0.85)
        .character(0, 0.55)
        .env(1, 0.01, 0.5, 0.15, 0.3)
        .env_to_cut(0.8)
        .amp(0.004, 0.0, 1.0, 0.25)
        .fx(delay(NoteDivision::Quarter, 0.3, 0.2))
        .out(8.4),
    Lead: "Growl Lead" => lead(WavetableId::Growl, 3_500.0, 0.04)
        .pos(A, 0.6)
        .uni(A, 2, 14.0)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 3_000.0, 0.5)
        .character(0, 0.6)
        .lfo(1, LfoWave::Sine, 0.18)
        .route(ModSource::Lfo(1), ModDest::OscPosition(A as u8), 0.5)
        .route(ModSource::Macro(2), ModDest::OscPosition(A as u8), 0.5)
        .mac(2, "Growl")
        .fx(drive_fx(DistortionCurve::Tube, 10.0, 0.3))
        .out(17.1),
    Lead: "Sub Lead" => lead(WavetableId::SubTri, 1_400.0, 0.06)
        .uni(A, 1, 0.0)
        .semis(A, -12)
        .osc(SUB, WavetableId::SubSine, -19.0)
        .semis(SUB, -24)
        .filter_route(SUB, FilterRoute::Bypass)
        .amp(0.006, 0.0, 1.0, 0.2)
        .fx(drive_fx(DistortionCurve::SoftClip, 6.0, 0.2))
        .out(4.9),

    // ----------------------------------------------------------------- Pad ---
    Pad: "Init Pad" => pad(WavetableId::Saw, 2_500.0, 0.6, 1.6).out(5.3),
    Pad: "Warm Analog" => pad(WavetableId::AnalogMorph, 1_400.0, 1.2, 2.2)
        .pos(A, 0.55)
        .uni(A, 4, 12.0)
        .osc(B, WavetableId::Triangle, -22.0)
        .semis(B, -12)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 1_800.0, 0.3)
        .character(0, 0.3)
        .out(13.4),
    Pad: "Glass" => pad(WavetableId::Glass, 12_000.0, 0.8, 1.4)
        .pos(A, 0.3)
        .uni(A, 3, 6.0)
        .osc(B, WavetableId::Sine, -26.0)
        .semis(B, 12)
        .lfo(0, LfoWave::Sine, 0.1)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.4)
        .fx(reverb(0.85, 0.45))
        .out(7.5),
    Pad: "Choir Pad" => choir(0.0, 0.6).out(24.0),
    Pad: "String Pad" => strings(2_200.0, 0.45, 3.0)
        .fx(ensemble(4, 0.45))
        .fx(reverb(0.7, 0.35))
        .out(3.3),
    Pad: "Dark Drone" => pad(WavetableId::Hollow, 420.0, 2.5, 5.0)
        .pos(A, 0.6)
        .uni(A, 2, 8.0)
        .osc(B, WavetableId::SubSaw, -20.0)
        .semis(B, -12)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 700.0, 1.2)
        .lfo(0, LfoWave::Sine, 0.05)
        .lfo_mode(0, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.3)
        .fx(reverb(0.9, 0.5))
        .out(7.5),
    Pad: "Shimmer" => pad(WavetableId::Sine, 12_000.0, 1.5, 2.0)
        .uni(A, 3, 4.0)
        .osc(B, WavetableId::Sine, -24.0)
        .semis(B, 19)
        .osc(C, WavetableId::Sine, -30.0)
        .semis(C, 24)
        .lfo(0, LfoWave::Sine, 0.13)
        .lfo(1, LfoWave::Sine, 0.17)
        .route(ModSource::Lfo(1), ModDest::LayerGain(B as u8), 0.3)
        .route(ModSource::Lfo(0), ModDest::LayerGain(C as u8), 0.3)
        .fx(reverb(0.85, 0.5))
        .fx(delay(NoteDivision::Quarter, 0.4, 0.2))
        .out(-2.2),
    Pad: "Evolving" => pad(WavetableId::FormantSweep, 3_000.0, 0.7, 1.4)
        .pos(A, 0.2)
        .uni(A, 3, 10.0)
        .lfo(0, LfoWave::Sine, 0.07)
        .lfo_mode(0, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.5)
        .lfo(1, LfoWave::Triangle, 0.11)
        .route(ModSource::Lfo(1), ModDest::FilterCutoff(0), 0.3)
        .out(13.8),
    Pad: "Wide Digital" => pad(WavetableId::Stairs, 9_000.0, 0.15, 0.6)
        .pos(A, 0.4)
        .uni(A, 6, 18.0)
        .width(A, 1.0)
        .fx(ensemble(4, 0.4))
        .out(1.5),
    Pad: "Vox Air" => choir(0.35, 2.2)
        .osc(A, WavetableId::Choir, -16.0)
        .pos(A, 0.5)
        .uni(A, 4, 9.0)
        .noise(0.7, -32.0)
        .out(33.7),
    Pad: "Filtered Saw" => pad(WavetableId::Saw, 1_000.0, 0.02, 1.4)
        .uni(A, 4, 12.0)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 1_000.0, 0.8)
        .env(1, 1.2, 2.0, 0.4, 1.0)
        .env_to_cut(0.5)
        .out(4.3),
    Pad: "Dream" => pad(WavetableId::Sine, 9_000.0, 0.05, 4.0)
        .uni(A, 2, 5.0)
        .off(B)
        .osc(B, WavetableId::Triangle, -22.0)
        .semis(B, 12)
        .lfo(0, LfoWave::Sine, 0.3)
        .route(ModSource::Lfo(0), ModDest::LayerPan(A as u8), 0.4)
        .fx(delay(NoteDivision::EighthDotted, 0.35, 0.3))
        .fx(reverb(0.8, 0.4))
        .out(1.7),

    Pad: "Halo" => pad(WavetableId::Sine, 8_000.0, 1.0, 3.0)
        .uni(A, 3, 6.0)
        .warp(A, WarpMode::Fm, 0.18)
        .modulator(A, C)
        .osc(C, WavetableId::Sine, SILENT_DB)
        .semis(C, 26)
        .osc(B, WavetableId::Sine, -26.0)
        .semis(B, 12)
        .env(1, 2.0, 3.0, 0.4, 2.0)
        .route(ModSource::Envelope(1), ModDest::OscWarp(A as u8), 0.3)
        .route(ModSource::Macro(2), ModDest::OscWarp(A as u8), 0.4)
        .mac(2, "Bell")
        .fx(reverb(0.9, 0.5))
        .out(4.9),
    Pad: "Bowed Glass" => pad(WavetableId::Hollow, 4_000.0, 1.4, 2.6)
        .pos(A, 0.35)
        .uni(A, 3, 8.0)
        .filter(0, FilterModel::Comb, SvfMode::Lowpass, 320.0, 0.5)
        .character(0, 0.7)
        .key_track(0, 1.0)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 6_000.0, 0.5)
        .out(8.1),
    Pad: "Ice Field" => pad(WavetableId::BrightStack, 14_000.0, 0.9, 2.4)
        .pos(A, 0.5)
        .uni(A, 4, 9.0)
        .off(B)
        .noise(0.95, -26.0)
        .filter_route(NOISE, FilterRoute::F2)
        .filter(1, FilterModel::Clean, SvfMode::Highpass, 5_000.0, 0.8)
        .lfo(1, LfoWave::Sine, 0.09)
        .lfo_mode(1, LfoMode::Free)
        .route(ModSource::Lfo(1), ModDest::LayerGain(NOISE as u8), 0.3)
        .route(ModSource::Macro(2), ModDest::LayerGain(NOISE as u8), 0.25)
        .mac(2, "Air")
        .fx(reverb(0.95, 0.5))
        .out(25.7),
    // A ladder in **band-pass**, swelling: the pad that is a formant rather
    // than a wall, which is the one shape a low-pass pad cannot reach.
    Pad: "Reso Swell" => pad(WavetableId::Saw, 600.0, 1.8, 3.0)
        .uni(A, 4, 10.0)
        .filter(0, FilterModel::Ladder, SvfMode::Bandpass, 400.0, 0.9)
        .character(0, 0.4)
        .env(1, 2.5, 2.0, 0.5, 2.0)
        .env_to_cut(0.75)
        .out(1.5),
    Pad: "Brass Pad" => brass(2_600.0, 0.5)
        .osc(A, WavetableId::BrightStack, -16.0)
        .pos(A, 0.5)
        .uni(A, 4, 12.0)
        .osc(C, WavetableId::Saw, -23.0)
        .semis(C, 12)
        .amp(0.5, 0.0, 1.0, 1.8)
        .env(1, 0.5, 1.0, 0.55, 1.2)
        .env_to_cut(0.65)
        .fx(ensemble(3, 0.35))
        .fx(reverb(0.75, 0.4))
        .out(11.9),
    Pad: "Metal Pad" => pad(WavetableId::Gong, 5_000.0, 1.2, 3.5)
        .pos(A, 0.4)
        .uni(A, 2, 5.0)
        .osc(B, WavetableId::Gong, -24.0)
        .semis(B, 7)
        .pos(B, 0.7)
        .lfo(1, LfoWave::Triangle, 0.06)
        .lfo_mode(1, LfoMode::Free)
        .route(ModSource::Lfo(1), ModDest::OscPosition(B as u8), 0.5)
        .route(ModSource::Macro(2), ModDest::OscPosition(A as u8), 0.5)
        .mac(2, "Metal")
        .fx(reverb(0.9, 0.45))
        .out(16.0),

    // ---------------------------------------------------------------- Keys ---
    // The FM e-pianos are here because two-operator FM is what a DX7 e-piano
    // *is*.
    //
    // A **sampled** grand is still the soundfont player's job, and this row
    // does not pretend otherwise. What it is, is the *physics* of one — and,
    // after two rounds of *"doesn't sound like a grand"*, the physics as
    // **measured** rather than as reasoned: every number below was set
    // against a sampled grand read through `fontelle-app`'s
    // `examples/piano_probe.rs`, and `tests/grand_piano.rs` holds the
    // readings as windows. The first pass had reasoned its way to a dark,
    // slow sine with a knock on it — a struck-string table at its softest
    // position under a lid, an amplitude envelope over a minute long, and a
    // high-passed click — which is exactly an electric piano with a
    // clavinet's attack, and that is what Ty heard.
    //
    // What the reference actually is, key by key:
    //
    // - **Rich.** At middle C its partials 2, 3 and 4 sit 2, 9 and 7 dB
    //   under the fundamental, and they are *still* within 13 dB a second
    //   later. The ring is a string, not a sine.
    // - **Richer at the bottom, purer at the top.** C2's second partial is
    //   8 dB *over* its fundamental (a short soundboard cannot radiate 65
    //   Hz); C6 and C7 are within a few dB of a sine.
    // - **Fast.** Thirty decibels go in 2.0 s at C2, 1.4 s at C4, 0.17 s at
    //   C7 — the prompt sound leaving — and what is left rings on quietly.
    // - **Quieter going up**: the top octave is ten decibels under the
    //   middle, and the bass a shade over it.
    Keys: "Grand Piano" => init()
        // **The aftersound.** A struck string (`WavetableId::Struck`), three
        // of them a cent apart with the outer two under the middle — a
        // piano's chorus is its own unison, and it is *slow*: a trichord
        // beats at a sixth of a hertz. Read at the **bright** end of the
        // table in the bass and darker up the keyboard: the position is the
        // hammer's hardness, and the same felt is harder against a short
        // stiff treble string than a long bass one. Its level eases off
        // with the key, and velocity opens it.
        //
        // Its gain starts twenty-nine decibels up and comes down on env 1 —
        // see the envelopes below for why the decay is two straight lines.
        // Hot, on purpose: the sine below has to sit ten decibels under
        // this in the middle and climb over it at the top, and a layer
        // cannot start below the floor, so the whole voice runs high and
        // the output trim takes it back down.
        .osc(A, WavetableId::Struck, -30.5)
        .pos(A, 0.0)
        .inverted(ModSource::Key, ModDest::OscPosition(A as u8), 0.9)
        .inverted(ModSource::Key, ModDest::LayerGain(A as u8), 0.05)
        .uni(A, 3, 1.2)
        .blend(A, 0.3)
        .locked(A)
        .width(A, 0.25)
        .filter_route(A, FilterRoute::F1)
        // **The prompt sound**: the same string, harder struck, on a fast
        // envelope of its own (env 3), at unison and phase-locked to the
        // one above so the two sum rather than beat. A few decibels over
        // the aftersound at a normal touch and well over it at a hard one,
        // which is what makes a strike — and what velocity mostly moves.
        .osc(B, WavetableId::Struck, SILENT_DB + 2.0)
        .pos(B, 0.62)
        .locked(B)
        .filter_route(B, FilterRoute::F1)
        .inverted(ModSource::Key, ModDest::LayerGain(B as u8), 0.05)
        // **A sine at the fundamental**, well under the strings in the
        // middle and *over* them at the top: a treble string is short and
        // stiff and very nearly a sine, and the reference's C7 has its
        // second partial twenty-four decibels down. Its level climbs
        // steeply with the key, which is the whole of the crossfade. It
        // rides the same fast slope the strings do (env 1): left off it, it
        // was what the note settled on after a second, and a piano's ring
        // is a string and not a sine. At unison, not an octave down,
        // because a piano has no sub.
        .osc(C, WavetableId::Sine, -60.0)
        .filter_route(C, FilterRoute::F1)
        .route(ModSource::Key, ModDest::LayerGain(C as u8), 0.612)
        // **The bass string's octave**, and nothing else's: a short
        // soundboard cannot radiate 65 Hz, so the reference's C2 has its
        // second partial eight decibels *over* its fundamental. The same
        // table an octave up, phase-locked so it sums with the string's own
        // second partial rather than beating against it, and gone by the
        // middle of the keyboard. **Not a sub**: a piano has none, and this
        // is the opposite direction.
        .osc(SUB, WavetableId::Struck, SILENT_DB + 2.0)
        .semis(SUB, 12)
        .pos(SUB, 0.6)
        .locked(SUB)
        .filter_route(SUB, FilterRoute::F1)
        .inverted(ModSource::Key, ModDest::LayerGain(SUB as u8), 0.362)
        // The hammer: a **thud**, not a knock. Dark noise through its own
        // low-pass, over in fifteen milliseconds and felt more than heard —
        // the first pass high-passed it into a click, which is a clavinet's
        // tangent. Just *above* the floor, because a layer at `SILENT_DB` is
        // skipped by the voice until a route lifts it, one block late.
        .noise(0.6, SILENT_DB + 2.0)
        .filter_route(NOISE, FilterRoute::F2)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 900.0, 0.3)
        // **The lid.** The strings' own spectrum is where the brightness
        // comes from, so the filter's job is the very top: at middle C the
        // reference's partials 9 to 12 sit near −30 dB where the table puts
        // them near −22, and at C6 and C7 the reference has almost no
        // second partial at all. So: a 24 dB corner that *closes* as the key
        // rises — a quarter of an octave per octave — and that closes again
        // **as the note rings**, on the same fast envelope the strings ride
        // (env 1, below). It rests at 1.2 kHz and opens two octaves over
        // that at the strike, which is the shine leaving before the note
        // does: a string's upper modes are damped first, and a lid that
        // stayed put would leave the ring as bright as the blow.
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 1_200.0, 0.2)
        .slope(0, FilterSlope::Db24)
        .key_track(0, -0.32)
        // **Quieter going up.** The reference's top octave is ten decibels
        // under middle C and its bass a shade over: a short string holds
        // less energy. On the voice as a whole, so it moves the strings and
        // the sine together and leaves their crossfade alone.
        .inverted(ModSource::Key, ModDest::Amp, 0.4)
        // **Two straight lines, added.** The reference at middle C is nearly
        // level for two hundred milliseconds, then falls at some twenty
        // decibels a second for a second or so, then at three for as long
        // as anybody listens. One bent curve cannot be that — a bend strong
        // enough for the tail is a cliff at the front — but two straight
        // decibel slopes summed are, exactly: the string's gain rides down
        // env 1 (linear, so a straight line in dB — a gain route reads an
        // envelope's *level* into decibels) over its first second and a
        // half, and the amplitude envelope underneath is a long straight
        // decibel decay that is all that is left after. Both stored for
        // the top key and stretched down the keyboard by inverted key
        // routes, so the whole shape follows the key: thirty decibels go in
        // about 1.3 s at middle C, 3.4 s at C2 and under half a second at
        // C7, against the reference's 1.4, 2.0 and 0.17. The hold is the
        // reference's level first two hundred milliseconds.
        .amp(0.002, 2.65, 0.0, 0.10)
        .hold(0, 0.02)
        .curve(0, EnvelopeCurve::Decibel, 0.0)
        .inverted(ModSource::Key, ModDest::EnvelopeStageTime(0, 2), 0.6)
        .inverted(ModSource::Key, ModDest::EnvelopeStageTime(0, 3), 1.0)
        // **Straight**, which the Init envelopes are not: they carry a bend
        // that front-loads the fall, and on a gain route that bend was the
        // whole note dropping ten decibels in its first tenth of a second.
        .env(1, 0.0, 0.13, 0.0, 0.3)
        .hold(1, 0.026)
        .curve(1, EnvelopeCurve::Linear, 0.0)
        .inverted(ModSource::Key, ModDest::EnvelopeStageTime(1, 2), 0.6)
        .inverted(ModSource::Key, ModDest::EnvelopeStageTime(1, 3), 0.85)
        .route(ModSource::Envelope(1), ModDest::LayerGain(A as u8), 0.3)
        .route(ModSource::Envelope(1), ModDest::LayerGain(C as u8), 0.3)
        .route(ModSource::Envelope(1), ModDest::LayerGain(SUB as u8), 0.3)
        .env_to_cut(0.28)
        // The hammer's gate: fifteen milliseconds, and nothing after it.
        .env(2, 0.0, 0.015, 0.0, 0.01)
        .route(ModSource::Envelope(2), ModDest::LayerGain(NOISE as u8), 0.462)
        // **The prompt sound's own envelope**, the same kind of line, and
        // steeper: a third of a second at C7, a second and a half at middle
        // C, three and a half in the bass.
        .env(3, 0.0, 0.12, 0.0, 0.2)
        .hold(3, 0.026)
        .curve(3, EnvelopeCurve::Linear, 0.0)
        .inverted(ModSource::Key, ModDest::EnvelopeStageTime(3, 2), 0.6)
        .inverted(ModSource::Key, ModDest::EnvelopeStageTime(3, 3), 0.85)
        .route(ModSource::Envelope(3), ModDest::LayerGain(B as u8), 0.392)
        // Velocity is a piano's whole vocabulary: harder is brighter and
        // harder rings the partials. The felt is the first of those — thrown
        // harder, it is harder, and the string rings partials a soft strike
        // never reaches — and the prompt string leans on it hardest.
        .route(ModSource::Velocity, ModDest::OscPosition(A as u8), 0.5)
        .route(ModSource::Velocity, ModDest::OscPosition(B as u8), 0.6)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.18)
        .route(ModSource::Velocity, ModDest::LayerGain(B as u8), 0.3)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::LayerGain(NOISE as u8), 0.15)
        .mac(0, "Brightness").mac(1, "Hammer")
        // A little of the instrument's own lid, not a hall: the reference's
        // top octave is thirty decibels down in a sixth of a second, which
        // no room with a tail would allow.
        .fx(reverb(0.2, 0.06))
        .out(-24.0),
    Keys: "EP Tine" => electric_piano(0.25, 2.0).fx(chorus(2, 0.2)).out(-1.9),
    Keys: "EP Soft" => electric_piano(0.08, 3.2)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 3_000.0, 0.5)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.3)
        .out(3.0),
    Keys: "EP Dirty" => electric_piano(0.5, 1.1)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 2_200.0, 0.55)
        .character(0, 0.7)
        .amp(0.001, 1.1, 0.1, 0.15)
        .fx(drive_fx(DistortionCurve::Tube, 12.0, 0.35))
        .fx(chorus(2, 0.25))
        .out(15.9),
    Keys: "Clav" => init()
        .osc(A, WavetableId::Pulse, -12.0)
        .pos(A, 0.85)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Highpass, 400.0, 0.7)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 8_000.0, 1.1)
        .amp(0.001, 0.7, 0.08, 0.05)
        .env(1, 0.0, 0.08, 0.0, 0.06)
        .env_to_cut(0.5)
        .route(ModSource::Velocity, ModDest::FilterCutoff(1), 0.4)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(1), 0.4)
        .route(ModSource::Macro(1), ModDest::OscPosition(A as u8), 0.4)
        .mac(0, "Tone").mac(1, "Width")
        .out(17.1),
    Keys: "Wurly" => init()
        .amp(0.004, 2.6, 0.45, 0.5)
        .osc(A, WavetableId::Triangle, -11.0)
        .warp(A, WarpMode::Bend, 0.3)
        .osc(B, WavetableId::Sine, -24.0)
        .semis(B, 12)
        .off(C).off(SUB)
        .no_filter()
        .amp(0.002, 1.5, 0.4, 0.3)
        .route(ModSource::Velocity, ModDest::OscWarp(A as u8), 0.4)
        .route(ModSource::Macro(0), ModDest::OscWarp(A as u8), 0.4)
        .lfo(1, LfoWave::Sine, 5.0)
        .lfo_depth(1, 0.0)
        .route(ModSource::Lfo(1), ModDest::Amp, 0.35)
        .route(ModSource::Macro(1), ModDest::LfoDepth(1), 1.0)
        .mac(0, "Bite").mac(1, "Tremolo")
        .fx(drive_fx(DistortionCurve::SoftClip, 8.0, 0.15))
        .fx(chorus(2, 0.2))
        .out(-1.0),
    Keys: "Synth Piano" => init()
        .osc(A, WavetableId::Saw, -14.0)
        .uni(A, 2, 4.0)
        .osc(B, WavetableId::Sine, -22.0)
        .semis(B, 12)
        .off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 3_000.0, 0.5)
        .key_track(0, 0.7)
        .amp(0.002, 3.0, 0.2, 0.25)
        .env(1, 0.0, 0.6, 0.0, 0.3)
        .env_to_cut(0.6)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.5)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::LayerGain(B as u8), 0.2)
        .mac(0, "Brightness").mac(1, "Bell")
        .out(1.6),
    Keys: "Harpsi" => pluck(WavetableId::Sawstack, 20_000.0, 1.2)
        .pos(A, 0.3)
        .osc(B, WavetableId::Saw, -19.0)
        .semis(B, 12)
        .filter(0, FilterModel::Clean, SvfMode::Highpass, 300.0, 0.7)
        .env(1, 0.0, 0.02, 0.0, 0.02)
        .route(ModSource::Envelope(1), ModDest::OscPosition(A as u8), 0.3)
        .amp(0.001, 1.2, 0.0, 0.04)
        .out(10.0),
    Keys: "Toy Piano" => bell(WavetableId::Tine, 0.7)
        .pos(A, 0.6)
        .osc(B, WavetableId::Sine, -21.0)
        .semis(B, 24)
        .fx(reverb(0.3, 0.25))
        .out(4.1),
    Keys: "Music Box" => bell(WavetableId::Glass, 1.5)
        .pos(A, 0.1)
        .osc(B, WavetableId::Sine, -27.0)
        .semis(B, 36)
        .fx(delay(NoteDivision::Eighth, 0.3, 0.15))
        .fx(reverb(0.5, 0.3))
        .out(6.7),
    Keys: "Digital Keys" => init()
        .osc(A, WavetableId::Bitwave, -14.0)
        .pos(A, 0.4)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 6_000.0, 0.5)
        .amp(0.002, 0.8, 0.5, 0.2)
        .env(1, 0.0, 0.3, 0.0, 0.2)
        .env_to_cut(0.4)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::OscPosition(A as u8), 0.6)
        .mac(0, "Tone").mac(1, "Wave")
        .fx(chorus(2, 0.2))
        .out(13.2),

    // The reed and its neighbour a few cents away: the musette beat *is* the
    // instrument, and one reed on its own is an organ.
    Keys: "Accordion" => init()
        .osc(A, WavetableId::Odd, -14.0)
        .uni(A, 2, 14.0)
        .osc(B, WavetableId::Pulse, -20.0)
        .pos(B, 0.35)
        .fine(B, 9.0)
        .off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 1_400.0, 0.5)
        .slope(0, FilterSlope::Db12)
        .amp(0.03, 0.0, 1.0, 0.08)
        .route(ModSource::Velocity, ModDest::LayerGain(A as u8), 0.1)
        .route(ModSource::Macro(0), ModDest::OscUnisonDetune(A as u8), 0.6)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.3)
        .mac(0, "Musette").mac(1, "Tone")
        .fx(reverb(0.4, 0.2))
        .out(1.2),
    Keys: "Melodica" => init()
        .osc(A, WavetableId::Square, -13.0)
        .off(B).off(C).off(SUB)
        .noise(0.35, -28.0)
        .filter_route(NOISE, FilterRoute::F2)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 3_200.0, 0.6)
        .key_track(0, 0.5)
        .filter(1, FilterModel::Clean, SvfMode::Bandpass, 2_200.0, 1.0)
        .amp(0.02, 0.25, 0.7, 0.1)
        .env(2, 0.0, 0.05, 0.0, 0.04)
        .route(ModSource::Envelope(2), ModDest::LayerGain(NOISE as u8), 0.4)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.25)
        .route(ModSource::Macro(0), ModDest::LayerGain(NOISE as u8), 0.2)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.3)
        .mac(0, "Breath").mac(1, "Tone")
        .out(8.3),
    Keys: "Clav Wah" => init()
        .osc(A, WavetableId::Pulse, -12.0)
        .pos(A, 0.7)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Ladder, SvfMode::Bandpass, 900.0, 0.8)
        .character(0, 0.5)
        .amp(0.001, 0.9, 0.1, 0.06)
        .lfo_sync(0, LfoWave::Triangle, NoteDivision::Quarter)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.7)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.25)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.5)
        .route(ModSource::Macro(1), ModDest::FilterResonance(0), 0.5)
        .mac(0, "Wah rate").mac(1, "Resonance")
        .fx(drive_fx(DistortionCurve::SoftClip, 6.0, 0.2))
        .out(13.7),
    Keys: "Electric Grand" => electric_piano(0.12, 4.0)
        .uni(A, 2, 4.0)
        .semis(B, 24)
        .amp(0.002, 3.5, 0.15, 0.4)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 5_000.0, 0.5)
        .key_track(0, 0.6)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.35)
        .fx(chorus(2, 0.15))
        .fx(reverb(0.4, 0.2))
        .out(-1.5),
    Keys: "Rhodes Bell" => electric_piano(0.45, 2.2)
        .semis(B, 31)
        .amp(0.002, 2.2, 0.12, 0.3)
        .env(2, 0.0, 0.7, 0.0, 0.3)
        .fx(delay(NoteDivision::Eighth, 0.25, 0.12))
        .fx(reverb(0.5, 0.25))
        .out(-1.7),
    // The bolt between the strings: a ring modulator at a ratio that is not a
    // harmonic, on a hammer's envelope.
    Keys: "Prepared" => init()
        .osc(A, WavetableId::Triangle, -12.0)
        .warp(A, WarpMode::Rm, 0.75)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 13)
        .off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 6_000.0, 0.5)
        .noise(0.0, -30.0)
        .filter_route(NOISE, FilterRoute::F1)
        .amp(0.001, 1.4, 0.0, 0.25)
        .env(2, 0.0, 0.02, 0.0, 0.02)
        .route(ModSource::Envelope(2), ModDest::LayerGain(NOISE as u8), 0.35)
        .route(ModSource::Velocity, ModDest::OscWarp(A as u8), 0.3)
        .route(ModSource::Macro(0), ModDest::OscWarp(A as u8), 0.5)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.3)
        .mac(0, "Metal").mac(1, "Tone")
        .out(8.6),
    Keys: "Poly Keys" => init()
        .osc(A, WavetableId::Saw, -14.0)
        .uni(A, 2, 6.0)
        .osc(B, WavetableId::Pulse, -18.0)
        .pos(B, 0.4)
        .off(C).off(SUB)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 2_400.0, 0.45)
        .character(0, 0.3)
        .amp(0.006, 1.6, 0.35, 0.3)
        .env(1, 0.0, 0.9, 0.2, 0.4)
        .env_to_cut(0.45)
        .lfo(1, LfoWave::Triangle, 0.35)
        .route(ModSource::Lfo(1), ModDest::OscPosition(B as u8), 0.3)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.35)
        .route(ModSource::Macro(1), ModDest::OscPosition(B as u8), 0.4)
        .mac(0, "Brightness").mac(1, "Width")
        .fx(chorus(3, 0.3))
        .out(9.3),

    // --------------------------------------------------------------- Pluck ---
    Pluck: "Init Pluck" => pluck(WavetableId::Triangle, 400.0, 0.45).uni(A, 2, 8.0).out(4.6),
    Pluck: "Kalimba" => init()
        .osc(A, WavetableId::Sine, -11.0)
        .warp(A, WarpMode::Fm, 0.2)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 31)
        .off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 5_000.0, 0.4)
        .amp(0.001, 0.9, 0.0, 0.2)
        .env(2, 0.0, 0.15, 0.0, 0.1)
        .route(ModSource::Envelope(2), ModDest::OscWarp(A as u8), 0.4)
        .route(ModSource::Velocity, ModDest::OscWarp(A as u8), 0.3)
        .route(ModSource::Macro(0), ModDest::OscWarp(A as u8), 0.4)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.3)
        .mac(0, "Bite").mac(1, "Tone")
        .out(7.8),
    Pluck: "Guitar-ish" => pluck(WavetableId::Sawstack, 4_000.0, 1.2)
        .pos(A, 0.5)
        .uni(A, 2, 6.0)
        .filter(0, FilterModel::Comb, SvfMode::Lowpass, 260.0, 0.35)
        .character(0, 0.65)
        .key_track(0, 1.0)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 4_000.0, 0.5)
        .out(14.3),
    Pluck: "Pizzicato" => pluck(WavetableId::Saw, 6_000.0, 0.22)
        .uni(A, 4, 14.0)
        .width(A, 0.7)
        .env(1, 0.0, 0.05, 0.0, 0.04)
        .env_to_cut(0.35)
        .fx(reverb(0.3, 0.25))
        .out(6.0),
    Pluck: "Harp" => pluck(WavetableId::Triangle, 3_000.0, 1.8)
        .uni(A, 1, 0.0)
        .osc(B, WavetableId::Saw, -21.0)
        .semis(B, 12)
        .key_track(0, 0.6)
        .fx(delay(NoteDivision::Sixteenth, 0.25, 0.1))
        .out(1.4),
    Pluck: "Marimba" => init()
        .amp(0.001, 0.9, 0.0, 0.25)
        .osc(A, WavetableId::Sine, -11.0)
        .osc(B, WavetableId::Sine, -14.0)
        .semis(B, 24)
        .off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 4_000.0, 0.4)
        .amp(0.001, 0.6, 0.0, 0.15)
        .env(2, 0.0, 0.08, 0.0, 0.05)
        .route(ModSource::Envelope(2), ModDest::LayerGain(B as u8), 0.15)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(0), ModDest::LayerGain(B as u8), 0.15)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.3)
        .mac(0, "Mallet").mac(1, "Tone")
        .out(-4.9),
    Pluck: "Chip Pluck" => chip(WavetableId::NesPulse50, 0.01)
        .amp(0.0, 0.09, 0.0, 0.005)
        .out(0.6),
    Pluck: "Steel" => pluck(WavetableId::Grit, 2_000.0, 0.7)
        .pos(A, 0.3)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 2_000.0, 0.8)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 6_000.0, 0.5)
        .out(24.9),
    Pluck: "Water Drop" => pluck(WavetableId::Sine, 20_000.0, 0.45)
        .uni(A, 1, 0.0)
        .no_filter()
        .amp(0.001, 0.45, 0.0, 0.15)
        .env(1, 0.0, 0.04, 0.0, 0.04)
        .route(ModSource::Envelope(1), ModDest::LayerPitch(A as u8), 0.25)
        .fx(delay(NoteDivision::Eighth, 0.3, 0.25))
        .fx(reverb(0.5, 0.3))
        .out(1.9),
    Pluck: "Dulcimer" => pluck(WavetableId::Sawstack, 9_000.0, 2.2)
        .pos(A, 0.2)
        .uni(A, 3, 3.0)
        .filter(0, FilterModel::Clean, SvfMode::Highpass, 220.0, 0.7)
        .env_to_cut(0.0)
        .fx(delay(NoteDivision::SixteenthDotted, 0.25, 0.15))
        .out(15.3),

    // The guitars. All of them are the same idea — a comb tuned to the note is
    // the string, the source is what plucked it — and they are told apart by
    // how long the comb rings and how much of the source survives it.
    Pluck: "Nylon" => pluck(WavetableId::Triangle, 2_600.0, 1.4)
        .uni(A, 1, 0.0)
        .osc(B, WavetableId::Saw, -24.0)
        .filter(0, FilterModel::Comb, SvfMode::Lowpass, 220.0, 0.4)
        .character(0, 0.5)
        .key_track(0, 1.0)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 2_600.0, 0.5)
        .out(2.1),
    Pluck: "Jazz Guitar" => pluck(WavetableId::Triangle, 1_100.0, 2.6)
        .uni(A, 1, 0.0)
        .osc(B, WavetableId::Saw, -22.0)
        .filter(0, FilterModel::Comb, SvfMode::Lowpass, 190.0, 0.3)
        .character(0, 0.85)
        .key_track(0, 1.0)
        .filter_route(A, FilterRoute::Serial)
        .filter_route(B, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 1_100.0, 0.5)
        .noise(0.3, -30.0)
        .filter_route(NOISE, FilterRoute::F2)
        .env(2, 0.0, 0.012, 0.0, 0.01)
        .route(ModSource::Envelope(2), ModDest::LayerGain(NOISE as u8), 0.35)
        .amp(0.002, 2.6, 0.0, 0.7)
        .out(-0.1),
    Pluck: "Banjo" => pluck(WavetableId::Grit, 6_000.0, 0.5)
        .pos(A, 0.6)
        .uni(A, 1, 0.0)
        .filter(0, FilterModel::Clean, SvfMode::Highpass, 500.0, 0.6)
        .env_to_cut(-0.3)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Comb, SvfMode::Lowpass, 240.0, 0.3)
        .character(1, 0.7)
        .key_track(1, 1.0)
        .noise(0.0, -24.0)
        .filter_route(NOISE, FilterRoute::F1)
        .env(2, 0.0, 0.01, 0.0, 0.01)
        .route(ModSource::Envelope(2), ModDest::LayerGain(NOISE as u8), 0.4)
        .amp(0.001, 0.5, 0.0, 0.12)
        .out(8.7),
    Pluck: "Koto" => pluck(WavetableId::Sawstack, 4_500.0, 1.1)
        .pos(A, 0.15)
        .uni(A, 1, 0.0)
        .filter(0, FilterModel::Comb, SvfMode::Lowpass, 300.0, 0.25)
        .character(0, 0.85)
        .key_track(0, 1.0)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 4_500.0, 0.6)
        .env(2, 0.0, 0.12, 0.0, 0.1)
        .route(ModSource::Envelope(2), ModDest::LayerPitch(A as u8), 0.012)
        .route(ModSource::Macro(2), ModDest::LayerPitch(A as u8), 0.02)
        .mac(2, "Bend")
        .out(11.1),
    // The sympathetic strings: a second course an octave up, through a
    // band-pass the played note is not damping. That is the buzz.
    Pluck: "Sitar" => pluck(WavetableId::Grit, 7_000.0, 1.6)
        .pos(A, 0.35)
        .uni(A, 2, 5.0)
        .osc(B, WavetableId::Sawstack, -22.0)
        .semis(B, 12)
        .filter(0, FilterModel::Comb, SvfMode::Lowpass, 280.0, 0.2)
        .character(0, 0.9)
        .key_track(0, 1.0)
        .filter_route(A, FilterRoute::Serial)
        .filter_route(B, FilterRoute::F2)
        .filter(1, FilterModel::Clean, SvfMode::Bandpass, 3_000.0, 0.9)
        .amp(0.001, 1.6, 0.0, 0.5)
        .fx(reverb(0.6, 0.3))
        .out(14.8),
    Pluck: "Ukulele" => pluck(WavetableId::Triangle, 5_000.0, 0.55)
        .semis(A, 12)
        .uni(A, 1, 0.0)
        .osc(B, WavetableId::Saw, -26.0)
        .semis(B, 12)
        .filter(0, FilterModel::Comb, SvfMode::Lowpass, 420.0, 0.35)
        .character(0, 0.45)
        .key_track(0, 1.0)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Highpass, 350.0, 0.6)
        .amp(0.001, 0.55, 0.0, 0.15)
        .out(4.6),
    Pluck: "Mandolin" => pluck(WavetableId::Sawstack, 6_500.0, 0.8)
        .pos(A, 0.4)
        .semis(A, 12)
        .uni(A, 2, 11.0)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 2_400.0, 0.7)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 7_000.0, 0.5)
        .lfo_sync(0, LfoWave::Triangle, NoteDivision::ThirtySecond)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.5)
        .route(ModSource::Macro(2), ModDest::LfoRate(0), 0.5)
        .mac(2, "Tremolo")
        .out(35.1),
    Pluck: "Muted" => pluck(WavetableId::Saw, 1_200.0, 0.18)
        .uni(A, 1, 0.0)
        .osc(SUB, WavetableId::SubSine, -20.0)
        .semis(SUB, -12)
        .filter_route(SUB, FilterRoute::Bypass)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 1_000.0, 0.35)
        .amp(0.001, 0.2, 0.0, 0.06)
        .env(1, 0.0, 0.05, 0.0, 0.04)
        .env_to_cut(0.35)
        .noise(0.2, -26.0)
        .filter_route(NOISE, FilterRoute::F1)
        .env(2, 0.0, 0.015, 0.0, 0.012)
        .route(ModSource::Envelope(2), ModDest::LayerGain(NOISE as u8), 0.35)
        .out(5.9),

    // ------------------------------------------------------------- Strings ---
    Strings: "Ensemble" => strings(3_500.0, 0.55, 1.4)
        .fx(ensemble(4, 0.5))
        .fx(reverb(0.6, 0.3))
        .out(3.0),
    Strings: "Solo Violin" => strings(20_000.0, 0.2, 0.35)
        .uni(A, 1, 0.0)
        .osc(B, WavetableId::Sawstack, -26.0)
        .semis(B, 12)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 1_000.0, 0.3)
        .character(0, 0.55)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 5_000.0, 0.5)
        .lfo(0, LfoWave::Sine, 6.0)
        .late(0, 0.2, 0.3)
        .mono(0.025)
        .out(7.7),
    Strings: "Cello" => strings(20_000.0, 0.15, 0.6)
        .uni(A, 1, 0.0)
        .semis(A, -12)
        .semis(B, -24)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 700.0, 0.3)
        .character(0, 0.75)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 2_500.0, 0.5)
        .mono(0.04)
        .out(14.3),
    Strings: "Staccato" => strings(3_500.0, 0.015, 0.12)
        .amp(0.015, 0.25, 0.3, 0.12)
        .fx(ensemble(4, 0.4))
        .out(6.9),
    Strings: "Tremolo Strings" => strings(3_200.0, 0.06, 0.5)
        .lfo_sync(1, LfoWave::Square, NoteDivision::ThirtySecond)
        .smooth(1, 0.12)
        .route(ModSource::Lfo(1), ModDest::Amp, 1.0)
        .fx(ensemble(3, 0.4))
        .out(-20.9),
    Strings: "Synth Strings" => strings(14_000.0, 0.5, 1.6)
        .osc(A, WavetableId::AnalogMorph, -17.0)
        .pos(A, 0.95)
        .uni(A, 6, 16.0)
        .width(A, 1.0)
        .osc(B, WavetableId::Square, -24.0)
        .semis(B, -12)
        .filter(0, FilterModel::Clean, SvfMode::Highpass, 300.0, 0.7)
        .slope(0, FilterSlope::Db12)
        .fx(ensemble(4, 0.45))
        .out(10.0),
    Strings: "Pizz Section" => strings(2_500.0, 0.001, 0.15)
        .amp(0.001, 0.28, 0.0, 0.1)
        .uni(A, 4, 14.0)
        .fx(reverb(0.7, 0.35))
        .out(7.4),
    Strings: "Baroque" => strings(2_500.0, 0.02, 0.18)
        .osc(A, WavetableId::Sawstack, -16.0)
        .pos(A, 0.3)
        .uni(A, 3, 8.0)
        .fx(reverb(0.5, 0.3))
        .out(8.1),

    Strings: "Viola" => strings(20_000.0, 0.18, 0.45)
        .uni(A, 1, 0.0)
        .semis(A, -7)
        .semis(B, -19)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 850.0, 0.3)
        .character(0, 0.65)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 3_500.0, 0.5)
        .mono(0.03)
        .out(11.8),
    Strings: "Double Bass" => strings(20_000.0, 0.12, 0.5)
        .uni(A, 1, 0.0)
        .semis(A, -24)
        .off(B)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 450.0, 0.35)
        .character(0, 0.85)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 1_400.0, 0.5)
        .mono(0.05)
        .out(17.2),
    Strings: "Quartet" => strings(9_000.0, 0.12, 0.7)
        .uni(A, 2, 5.0)
        .osc(B, WavetableId::Sawstack, -20.0)
        .pos(B, 0.4)
        .semis(B, -12)
        .uni(B, 2, 4.0)
        .filter(0, FilterModel::Clean, SvfMode::Highpass, 180.0, 0.6)
        .slope(0, FilterSlope::Db12)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 6_000.0, 0.5)
        .fx(reverb(0.45, 0.25))
        .out(6.2),
    Strings: "Sordino" => strings(700.0, 0.4, 1.2)
        .uni(A, 3, 8.0)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 700.0, 0.3)
        .fx(ensemble(3, 0.35))
        .fx(reverb(0.7, 0.35))
        .out(8.2),
    Strings: "Marcato" => strings(5_000.0, 0.005, 0.45)
        .amp(0.005, 0.35, 0.55, 0.4)
        .env(1, 0.0, 0.12, 0.3, 0.2)
        .env_to_cut(0.55)
        .uni(A, 4, 11.0)
        .fx(ensemble(3, 0.4))
        .out(4.9),
    Strings: "Slow Strings" => strings(5_000.0, 1.8, 3.4)
        .uni(A, 6, 16.0)
        .width(A, 1.0)
        .osc(B, WavetableId::Sawstack, -22.0)
        .pos(B, 0.6)
        .semis(B, -12)
        .fx(ensemble(4, 0.5))
        .fx(reverb(0.85, 0.45))
        .out(12.2),

    // ------------------------------------------------------ Brass & Winds ---
    BrassAndWinds: "Brass Section" => brass(1_200.0, 0.03)
        .fx(chorus(2, 0.2))
        .fx(reverb(0.5, 0.25))
        .out(6.7),
    BrassAndWinds: "Solo Trumpet" => brass(20_000.0, 0.02)
        .uni(A, 1, 0.0)
        .off(B)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 1_300.0, 0.25)
        .character(0, 0.15)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 4_000.0, 0.5)
        .mono(0.02)
        .lfo(0, LfoWave::Sine, 5.5)
        .late(0, 0.4, 0.3)
        .route_via(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.001, ModSource::ModWheel)
        .out(26.5),
    BrassAndWinds: "French Horn" => brass(600.0, 0.22)
        .osc(B, WavetableId::Triangle, -20.0)
        .amp(0.22, 0.0, 1.0, 0.7)
        .env_to_cut(0.2)
        .fx(reverb(0.7, 0.35))
        .out(5.4),
    BrassAndWinds: "Synth Brass" => brass(4_500.0, 0.005)
        .uni(A, 5, 18.0)
        .osc(C, WavetableId::Square, -26.0)
        .semis(C, 12)
        .character(0, 0.4)
        .env(1, 0.02, 0.3, 0.5, 0.2)
        .env_to_cut(0.8)
        .fx(chorus(3, 0.25))
        .out(6.2),
    BrassAndWinds: "Flute" => init()
        .osc(A, WavetableId::Sine, -12.0)
        .osc(B, WavetableId::Triangle, -28.0)
        .semis(B, 12)
        .off(C).off(SUB)
        .noise(0.25, -42.0)
        .filter_route(NOISE, FilterRoute::F2)
        .filter(1, FilterModel::Clean, SvfMode::Bandpass, 2_500.0, 1.2)
        .no_filter()
        .filter(1, FilterModel::Clean, SvfMode::Bandpass, 2_500.0, 1.2)
        .amp(0.07, 0.0, 1.0, 0.2)
        .lfo(0, LfoWave::Sine, 5.0)
        .late(0, 0.3, 0.3)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.15)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.000_5)
        .route(ModSource::Velocity, ModDest::LayerGain(NOISE as u8), 0.12)
        .route(ModSource::Macro(0), ModDest::LayerGain(NOISE as u8), 0.15)
        .route(ModSource::Macro(1), ModDest::LfoDepth(0), 0.6)
        .mac(0, "Breath").mac(1, "Vibrato")
        .fx(reverb(0.5, 0.3))
        .out(10.2),
    BrassAndWinds: "Clarinet" => init()
        .osc(A, WavetableId::Square, -13.0)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 2_500.0, 0.5)
        .key_track(0, 0.8)
        .amp(0.05, 0.0, 1.0, 0.15)
        .env(1, 0.05, 0.2, 0.6, 0.15)
        .env_to_cut(0.3)
        .mono(0.02)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.25)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::LfoDepth(0), 0.6)
        .lfo(0, LfoWave::Sine, 5.0)
        .lfo_depth(0, 0.0)
        .late(0, 0.3, 0.3)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.001)
        .mac(0, "Tone").mac(1, "Vibrato")
        .out(4.3),
    BrassAndWinds: "Oboe" => init()
        .osc(A, WavetableId::Pulse, -13.0)
        .pos(A, 0.2)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 1_000.0, 0.4)
        .character(0, 0.45)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 4_000.0, 0.5)
        .amp(0.04, 0.0, 1.0, 0.15)
        .lfo(0, LfoWave::Sine, 5.5)
        .late(0, 0.25, 0.3)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.000_6)
        .route(ModSource::Velocity, ModDest::FilterCharacter(0), 0.08)
        .route(ModSource::Macro(0), ModDest::FilterCharacter(0), 0.3)
        .route(ModSource::Macro(1), ModDest::LfoDepth(0), 0.6)
        .mac(0, "Reed").mac(1, "Vibrato")
        .out(10.4),
    BrassAndWinds: "Pan Pipe" => init()
        .osc(A, WavetableId::Sine, -12.0)
        .off(B).off(C).off(SUB)
        .noise(0.15, -37.0)
        .filter_route(NOISE, FilterRoute::F2)
        .filter(1, FilterModel::Clean, SvfMode::Bandpass, 1_800.0, 1.2)
        .amp(0.035, 0.8, 0.6, 0.2)
        .env(2, 0.0, 0.06, 0.0, 0.05)
        .route(ModSource::Envelope(2), ModDest::LayerGain(NOISE as u8), 0.5)
        .route(ModSource::Velocity, ModDest::LayerGain(NOISE as u8), 0.15)
        .route(ModSource::Macro(0), ModDest::LayerGain(NOISE as u8), 0.2)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(1), 0.3)
        .mac(0, "Chiff").mac(1, "Air")
        .fx(delay(NoteDivision::Eighth, 0.25, 0.2))
        .out(8.6),

    BrassAndWinds: "Trombone" => brass(900.0, 0.06)
        .uni(A, 1, 0.0)
        .semis(A, -12)
        .osc(B, WavetableId::Saw, -24.0)
        .semis(B, -24)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 900.0, 0.4)
        .character(0, 0.45)
        .amp(0.06, 0.0, 1.0, 0.3)
        .mono(0.06)
        .fx(reverb(0.55, 0.3))
        .out(13.8),
    BrassAndWinds: "Tuba" => brass(420.0, 0.09)
        .uni(A, 1, 0.0)
        .semis(A, -24)
        .off(B)
        .osc(SUB, WavetableId::SubSine, -20.0)
        .semis(SUB, -24)
        .filter_route(SUB, FilterRoute::Bypass)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 420.0, 0.35)
        .amp(0.09, 0.0, 1.0, 0.3)
        .mono(0.05)
        .out(8.0),
    // A reed is a pulse through a throat, and the growl is noise inside the
    // same throat rather than beside it — which is why the noise goes to F1.
    BrassAndWinds: "Alto Sax" => init()
        .osc(A, WavetableId::Pulse, -13.0)
        .pos(A, 0.35)
        .off(B).off(C).off(SUB)
        .noise(0.4, -30.0)
        .filter_route(NOISE, FilterRoute::F1)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 1_150.0, 0.35)
        .character(0, 0.3)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Ladder, SvfMode::Lowpass, 3_200.0, 0.4)
        .character(1, 0.5)
        .amp(0.035, 0.0, 1.0, 0.18)
        .env(1, 0.03, 0.2, 0.6, 0.2)
        .mono(0.03)
        .lfo(0, LfoWave::Sine, 5.2)
        .late(0, 0.3, 0.35)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.000_7)
        .route(ModSource::Velocity, ModDest::FilterCutoff(1), 0.3)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(1), 0.3)
        .route(ModSource::Macro(1), ModDest::LayerGain(NOISE as u8), 0.2)
        .mac(0, "Brightness").mac(1, "Breath")
        .fx(reverb(0.5, 0.25))
        .out(17.2),
    BrassAndWinds: "Tenor Sax" => init()
        .osc(A, WavetableId::Growl, -13.0)
        .pos(A, 0.3)
        .semis(A, -12)
        .off(B).off(C).off(SUB)
        .noise(0.5, -33.0)
        .filter_route(NOISE, FilterRoute::F1)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 780.0, 0.4)
        .character(0, 0.55)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 2_400.0, 0.5)
        .amp(0.045, 0.0, 1.0, 0.22)
        .mono(0.04)
        .lfo(0, LfoWave::Sine, 4.8)
        .late(0, 0.35, 0.4)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.000_8)
        .route(ModSource::Velocity, ModDest::FilterCharacter(0), 0.1)
        .route(ModSource::Macro(0), ModDest::FilterCharacter(0), 0.4)
        .route(ModSource::Macro(1), ModDest::LfoDepth(0), 0.6)
        .mac(0, "Vowel").mac(1, "Vibrato")
        .fx(drive_fx(DistortionCurve::Tube, 8.0, 0.2))
        .out(21.5),
    BrassAndWinds: "Bassoon" => init()
        .osc(A, WavetableId::Pulse, -13.0)
        .pos(A, 0.15)
        .semis(A, -12)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 1_300.0, 0.6)
        .key_track(0, 0.7)
        .amp(0.05, 0.0, 1.0, 0.16)
        .env(1, 0.04, 0.25, 0.6, 0.15)
        .env_to_cut(0.3)
        .mono(0.03)
        .lfo(0, LfoWave::Sine, 4.5)
        .lfo_depth(0, 0.0)
        .late(0, 0.35, 0.35)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.000_8)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.25)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::LfoDepth(0), 0.6)
        .mac(0, "Tone").mac(1, "Vibrato")
        .out(16.5),
    BrassAndWinds: "Piccolo" => init()
        .osc(A, WavetableId::Sine, -12.0)
        .semis(A, 24)
        .osc(B, WavetableId::Triangle, -30.0)
        .semis(B, 36)
        .off(C).off(SUB)
        .noise(0.3, -38.0)
        .filter_route(NOISE, FilterRoute::F2)
        .no_filter()
        .filter(1, FilterModel::Clean, SvfMode::Bandpass, 6_000.0, 1.2)
        .amp(0.03, 0.0, 1.0, 0.12)
        .lfo(0, LfoWave::Sine, 6.0)
        .late(0, 0.25, 0.25)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.000_6)
        .route(ModSource::Velocity, ModDest::LayerGain(NOISE as u8), 0.15)
        .route(ModSource::Macro(0), ModDest::LayerGain(NOISE as u8), 0.2)
        .route(ModSource::Macro(1), ModDest::LfoDepth(0), 0.6)
        .mac(0, "Breath").mac(1, "Vibrato")
        .out(9.0),
    BrassAndWinds: "Muted Trumpet" => brass(20_000.0, 0.03)
        .uni(A, 1, 0.0)
        .off(B)
        .filter(0, FilterModel::Clean, SvfMode::Highpass, 900.0, 0.8)
        .env_to_cut(-0.3)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Clean, SvfMode::Bandpass, 2_800.0, 1.4)
        .amp(0.02, 0.0, 1.0, 0.15)
        .mono(0.02)
        .out(16.5),
    // The breath is half the instrument, and the note arrives *under* pitch and
    // rises into it — a shakuhachi that starts in tune is a recorder.
    BrassAndWinds: "Shakuhachi" => init()
        .osc(A, WavetableId::Sine, -12.0)
        .osc(B, WavetableId::Triangle, -26.0)
        .semis(B, 12)
        .off(C).off(SUB)
        .noise(0.2, -36.0)
        .filter_route(NOISE, FilterRoute::F2)
        .no_filter()
        .filter(1, FilterModel::Clean, SvfMode::Bandpass, 1_600.0, 0.9)
        .amp(0.09, 0.0, 1.0, 0.3)
        .env(2, 0.0, 0.12, 0.0, 0.1)
        .route(ModSource::Envelope(2), ModDest::LayerPitch(A as u8), -0.008)
        .route(ModSource::Envelope(2), ModDest::LayerGain(NOISE as u8), 0.35)
        .lfo(0, LfoWave::Sine, 4.2)
        .late(0, 0.5, 0.6)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.001_5)
        .route(ModSource::Velocity, ModDest::LayerGain(NOISE as u8), 0.2)
        .route(ModSource::Macro(0), ModDest::LayerGain(NOISE as u8), 0.25)
        .route(ModSource::Macro(1), ModDest::LfoDepth(0), 0.6)
        .mac(0, "Breath").mac(1, "Vibrato")
        .fx(reverb(0.6, 0.3))
        .out(9.1),

    // ------------------------------------------------------ Choir & Vocal ---
    ChoirAndVocal: "Choir Ahh" => choir(0.0, 0.45).out(23.7),
    ChoirAndVocal: "Choir Ooh" => choir(1.0, 0.9).out(34.2),
    ChoirAndVocal: "Choir Mmm" => choir(0.9, 0.55)
        .filter(1, FilterModel::Clean, SvfMode::Lowpass, 1_500.0, 0.6)
        .filter_route(NOISE, FilterRoute::F2)
        .out(16.2),
    ChoirAndVocal: "Vowel Morph" => choir(0.5, 0.15)
        .lfo(1, LfoWave::Sine, 0.08)
        .lfo_mode(1, LfoMode::Free)
        .route(ModSource::Lfo(1), ModDest::FilterCharacter(0), 0.5)
        .out(27.8),
    ChoirAndVocal: "Boys Choir" => choir(0.1, 0.25)
        .semis(A, 12)
        .semis(B, 0)
        // A smaller throat: the formants themselves move up, which is what
        // makes a child's voice a child's voice rather than a transposed
        // adult's.
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 1_700.0, 0.8)
        .character(0, 0.1)
        .amp(0.25, 0.0, 1.0, 0.5)
        .out(18.7),
    ChoirAndVocal: "Synth Vox" => init()
        .osc(A, WavetableId::Vowel, -15.0)
        .pos(A, 0.3)
        .uni(A, 3, 8.0)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 5_000.0, 0.5)
        .amp(0.15, 0.0, 1.0, 0.5)
        .lfo(0, LfoWave::Sine, 0.2)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.4)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.2)
        .route(ModSource::Macro(0), ModDest::OscPosition(A as u8), 0.6)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.3)
        .mac(0, "Vowel").mac(1, "Brightness")
        .fx(chorus(3, 0.3))
        .fx(delay(NoteDivision::Quarter, 0.3, 0.2))
        .out(7.4),
    ChoirAndVocal: "Whisper" => init()
        .off(A).off(B).off(C).off(SUB)
        .noise(0.3, -6.0)
        .filter_route(NOISE, FilterRoute::F1)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 1_000.0, 0.7)
        .character(0, 0.3)
        .amp(0.2, 0.0, 1.0, 0.4)
        .route(ModSource::Velocity, ModDest::LayerGain(NOISE as u8), 0.15)
        .route(ModSource::Macro(0), ModDest::FilterCharacter(0), 0.9)
        .route(ModSource::Macro(1), ModDest::FilterResonance(0), 0.4)
        .mac(0, "Vowel").mac(1, "Focus")
        .fx(reverb(0.7, 0.35))
        .out(-7.8),
    ChoirAndVocal: "Robot Voice" => init()
        .osc(A, WavetableId::Saw, -14.0)
        .uni(A, 2, 6.0)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 1_000.0, 0.6)
        .filter_route(A, FilterRoute::Serial)
        .filter(1, FilterModel::Comb, SvfMode::Lowpass, 300.0, 0.3)
        .character(1, 0.6)
        .amp(0.005, 0.0, 1.0, 0.15)
        .lfo_sync(0, LfoWave::SampleHold, NoteDivision::Eighth)
        .smooth(0, 0.2)
        .route(ModSource::Lfo(0), ModDest::FilterCharacter(0), 1.0)
        .route(ModSource::Velocity, ModDest::FilterCutoff(1), 0.2)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.5)
        .route(ModSource::Macro(1), ModDest::FilterCharacter(1), 0.4)
        .mac(0, "Step rate").mac(1, "Metal")
        .fx(crush(6.0, 8_000.0, 0.5))
        .out(4.8),

    ChoirAndVocal: "Soprano" => choir(0.2, 0.3)
        .uni(A, 1, 0.0)
        .semis(A, 12)
        .off(B)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 1_500.0, 0.75)
        .character(0, 0.2)
        .amp(0.12, 0.0, 1.0, 0.6)
        .lfo(0, LfoWave::Sine, 5.5)
        .late(0, 0.35, 0.4)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.001_5)
        .mono(0.03)
        .out(37.3),
    ChoirAndVocal: "Baritone" => choir(0.75, 0.35)
        .uni(A, 1, 0.0)
        .semis(A, -12)
        .off(B)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 620.0, 0.8)
        .character(0, 0.8)
        .mono(0.04)
        .lfo(0, LfoWave::Sine, 5.0)
        .late(0, 0.4, 0.5)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.001_5)
        .out(43.0),
    ChoirAndVocal: "Gregorian" => choir(0.55, 1.4)
        .uni(A, 4, 7.0)
        .semis(A, -12)
        .osc(C, WavetableId::Choir, -22.0)
        .semis(C, -24)
        .pos(C, 0.6)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 700.0, 0.6)
        .amp(1.4, 0.0, 1.0, 2.0)
        .out(31.7),
    ChoirAndVocal: "Vocal Stab" => choir(0.15, 0.02)
        .uni(A, 3, 9.0)
        .amp(0.01, 0.35, 0.0, 0.12)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 1_100.0, 0.8)
        .env(1, 0.0, 0.1, 0.0, 0.08)
        .env_to_cut(0.3)
        .fx(delay(NoteDivision::Eighth, 0.3, 0.2))
        .out(26.0),

    // --------------------------------------------------------------- Organ ---
    Organ: "Drawbar 888" => organ(0.9, 6.5)
        .fx(drive_fx(DistortionCurve::Tube, 8.0, 0.2))
        .fx(reverb(0.4, 0.2))
        .out(1.1),
    Organ: "Drawbar Jazz" => organ(0.45, 0.8).out(2.4),
    Organ: "Church" => organ(0.7, 0.2)
        .osc(B, WavetableId::Sine, -18.0)
        .semis(B, -12)
        .osc(C, WavetableId::Sine, -26.0)
        .semis(C, 19)
        .amp(0.06, 0.0, 1.0, 0.6)
        .fx(reverb(0.95, 0.5))
        .out(1.7),
    Organ: "Combo" => organ(0.2, 6.0)
        .osc(A, WavetableId::Square, -14.0)
        .osc(B, WavetableId::Square, -20.0)
        .semis(B, 12)
        .osc(C, WavetableId::Square, -26.0)
        .semis(C, 24)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 4_000.0, 0.5)
        .filter_route(A, FilterRoute::F1)
        .amp(0.003, 0.0, 1.0, 0.03)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.15)
        .out(-4.9),
    Organ: "Farfisa" => organ(0.35, 6.0)
        .osc(A, WavetableId::Pulse, -14.0)
        .pos(A, 0.4)
        .uni(A, 2, 3.0)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 5_000.0, 0.5)
        .filter_route(A, FilterRoute::F1)
        .amp(0.002, 0.0, 1.0, 0.02)
        .out(2.5),
    Organ: "Percussive" => organ(0.3, 5.5)
        // The percussion tab: a second harmonic struck on every key and gone
        // in a quarter of a second, which is the whole of what the tab did.
        .osc(B, WavetableId::Sine, -34.0)
        .semis(B, 19)
        .amp(0.002, 0.0, 1.0, 0.03)
        .env(1, 0.0, 0.22, 0.0, 0.2)
        .route(ModSource::Envelope(1), ModDest::LayerGain(B as u8), 0.22)
        .out(2.7),

    Organ: "Rock Organ" => organ(0.8, 7.2)
        .osc(B, WavetableId::Sine, -19.0)
        .semis(B, 19)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 3_500.0, 0.5)
        .drive(0, 0.85)
        .character(0, 0.85)
        .filter_route(A, FilterRoute::F1)
        .filter_route(B, FilterRoute::F1)
        .amp(0.003, 0.0, 1.0, 0.05)
        .fx(drive_fx(DistortionCurve::Tube, 22.0, 0.6))
        .fx(reverb(0.4, 0.2))
        .out(-17.4),
    Organ: "Gospel" => organ(0.75, 6.8)
        .semis(A, -12)
        .osc(B, WavetableId::Sine, -16.0)
        .semis(B, 7)
        .osc(C, WavetableId::Sine, -20.0)
        .semis(C, 19)
        .route(ModSource::Velocity, ModDest::Amp, 0.12)
        .noise(0.0, -44.0)
        .env(2, 0.0, 0.006, 0.0, 0.006)
        .amp(0.002, 0.0, 1.0, 0.03)
        .fx(drive_fx(DistortionCurve::Tube, 12.0, 0.3))
        .out(5.6),
    // A flue pipe is a sine with wind in front of it: no drawbars, no Leslie,
    // and the room is half the instrument.
    Organ: "Pipe Flute" => organ(0.0, 0.15)
        .osc(A, WavetableId::Sine, -11.0)
        .osc(B, WavetableId::Sine, -26.0)
        .semis(B, 19)
        .noise(0.4, -34.0)
        .env(2, 0.0, 0.05, 0.0, 0.04)
        .amp(0.08, 0.0, 1.0, 0.25)
        .fx(reverb(0.95, 0.5))
        .out(13.8),
    Organ: "Reed Organ" => organ(0.5, 0.9)
        .osc(A, WavetableId::Odd, -13.0)
        .osc(B, WavetableId::Pulse, -21.0)
        .pos(B, 0.3)
        .fine(B, 7.0)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 3_000.0, 0.5)
        .filter_route(A, FilterRoute::F1)
        .amp(0.04, 0.0, 1.0, 0.12)
        .out(-2.3),
    Organ: "Theatre" => organ(0.05, 0.6)
        .osc(A, WavetableId::Sine, -8.0)
        .osc(B, WavetableId::Sine, -19.0)
        .semis(B, -12)
        .osc(C, WavetableId::Triangle, -27.0)
        .semis(C, 12)
        .route(ModSource::Velocity, ModDest::Amp, 0.12)
        .noise(0.0, -42.0)
        .amp(0.03, 0.0, 1.0, 0.2)
        .lfo(1, LfoWave::Sine, 6.5)
        .lfo_depth(1, 1.0)
        .route(ModSource::Lfo(1), ModDest::Amp, 0.6)
        .route(ModSource::Lfo(1), ModDest::LayerPitch(A as u8), 0.001_2)
        .route(ModSource::Macro(2), ModDest::LfoDepth(1), 1.0)
        .mac(2, "Tremulant")
        .fx(reverb(0.8, 0.4))
        .out(11.7),
    Organ: "Bass Pedals" => organ(0.85, 0.2)
        .semis(A, -24)
        .osc(B, WavetableId::Sine, -18.0)
        .semis(B, -12)
        .route(ModSource::Velocity, ModDest::Amp, 0.12)
        .noise(0.0, -40.0)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 900.0, 0.5)
        .filter_route(A, FilterRoute::F1)
        .amp(0.01, 0.0, 1.0, 0.12)
        .out(2.0),

    // ---------------------------------------------------- Bells & Mallets ---
    BellsAndMallets: "Tubular" => bell(WavetableId::FmBell, 4.0)
        .osc(B, WavetableId::Sine, -23.0)
        .semis(B, 19)
        .fx(reverb(0.8, 0.35))
        .out(6.7),
    BellsAndMallets: "Glockenspiel" => bell(WavetableId::Sine, 1.5)
        .osc(B, WavetableId::Sine, -25.0)
        .semis(B, 31)
        .osc(C, WavetableId::Sine, -31.0)
        .semis(C, 43)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 12_000.0, 0.4)
        .fx(reverb(0.5, 0.3))
        .out(0.6),
    BellsAndMallets: "Vibraphone" => bell(WavetableId::Sine, 2.5)
        .osc(B, WavetableId::Sine, -23.0)
        .semis(B, 24)
        .lfo(1, LfoWave::Sine, 5.0)
        .route(ModSource::Lfo(1), ModDest::Amp, 0.5)
        .fx(reverb(0.5, 0.3))
        .out(0.1),
    BellsAndMallets: "Celesta" => bell(WavetableId::Tine, 0.55)
        .pos(A, 0.2)
        .osc(B, WavetableId::Sine, -27.0)
        .semis(B, 36)
        .out(5.1),
    BellsAndMallets: "Gong" => bell(WavetableId::Gong, 6.0)
        .pos(A, 0.6)
        .uni(A, 2, 4.0)
        .amp(0.02, 6.0, 0.0, 3.0)
        .lfo(0, LfoWave::Sine, 0.4)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.3)
        .fx(reverb(0.9, 0.4))
        .out(12.3),
    BellsAndMallets: "Steel Drum" => bell(WavetableId::Sine, 0.8)
        .warp(A, WarpMode::Bend, 0.2)
        .osc(B, WavetableId::Sine, -19.0)
        .semis(B, 7)
        .osc(C, WavetableId::Sine, -25.0)
        .semis(C, 12)
        .route(ModSource::Velocity, ModDest::OscWarp(A as u8), 0.3)
        .out(-0.9),
    BellsAndMallets: "Crystal" => bell(WavetableId::Glass, 3.0)
        .pos(A, 0.8)
        .osc(B, WavetableId::Sine, -27.0)
        .semis(B, 24)
        .amp(0.01, 3.0, 0.0, 1.5)
        .fx(delay(NoteDivision::EighthDotted, 0.35, 0.25))
        .fx(reverb(0.7, 0.35))
        .out(7.9),
    BellsAndMallets: "Chime Tree" => bell(WavetableId::Glass, 2.0)
        .pos(A, 0.5)
        .uni(A, 4, 20.0)
        .lfo(0, LfoWave::Sine, 0.23)
        .lfo(1, LfoWave::Sine, 0.31)
        .route(ModSource::Lfo(0), ModDest::LayerGain(A as u8), 0.15)
        .route(ModSource::Lfo(1), ModDest::LayerPan(A as u8), 0.4)
        .fx(reverb(0.8, 0.4))
        .out(8.6),

    BellsAndMallets: "Xylophone" => bell(WavetableId::Sine, 0.28)
        .osc(B, WavetableId::Sine, -17.0)
        .semis(B, 19)
        .filter(0, FilterModel::Clean, SvfMode::Highpass, 3_000.0, 0.7)
        .filter_route(A, FilterRoute::Bypass)
        .filter_route(B, FilterRoute::Bypass)
        .noise(0.0, -26.0)
        .filter_route(NOISE, FilterRoute::F1)
        .env(3, 0.0, 0.008, 0.0, 0.008)
        .route(ModSource::Envelope(3), ModDest::LayerGain(NOISE as u8), 0.4)
        .amp(0.001, 0.28, 0.0, 0.1)
        .out(0.9),
    BellsAndMallets: "Hand Bells" => bell(WavetableId::FmBell, 1.6)
        .pos(A, 0.25)
        .osc(B, WavetableId::Sine, -20.0)
        .semis(B, 12)
        .osc(C, WavetableId::Sine, -26.0)
        .semis(C, 26)
        .amp(0.003, 1.6, 0.0, 0.6)
        .fx(reverb(0.55, 0.3))
        .out(4.8),
    BellsAndMallets: "Temple Bell" => bell(WavetableId::Gong, 8.0)
        .pos(A, 0.25)
        .semis(A, -12)
        .warp(A, WarpMode::Rm, 0.35)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 15)
        .amp(0.004, 8.0, 0.0, 4.0)
        .fx(reverb(0.95, 0.5))
        .out(14.1),
    BellsAndMallets: "Carillon" => bell(WavetableId::FmBell, 3.5)
        .pos(A, 0.7)
        .semis(A, -12)
        .uni(A, 2, 6.0)
        .osc(B, WavetableId::FmBell, -21.0)
        .semis(B, 15)
        .pos(B, 0.4)
        .amp(0.004, 3.5, 0.0, 1.6)
        .fx(reverb(0.9, 0.45))
        .out(7.5),
    BellsAndMallets: "Crotales" => bell(WavetableId::Sine, 2.2)
        .semis(A, 24)
        .osc(B, WavetableId::Sine, -19.0)
        .semis(B, 36)
        .amp(0.001, 2.2, 0.0, 1.0)
        .fx(reverb(0.7, 0.4))
        .out(-1.3),
    // Bowed rather than struck: the one on this shelf whose attack is slower
    // than its decay, which is what a wet finger on a rim actually is.
    BellsAndMallets: "Glass Harp" => bell(WavetableId::Glass, 2.5)
        .pos(A, 0.45)
        .uni(A, 2, 4.0)
        .osc(B, WavetableId::Sine, -22.0)
        .semis(B, 19)
        .noise(0.9, -38.0)
        .amp(0.9, 0.0, 1.0, 1.4)
        .fx(reverb(0.85, 0.45))
        .out(3.7),

    // ------------------------------------------------------- Chip & Retro ---
    ChipAndRetro: "NES Lead" => chip(WavetableId::NesPulse50, 0.005).out(-5.7),
    ChipAndRetro: "NES Pulse 25" => chip(WavetableId::NesPulse25, 0.005)
        .amp(0.0, 0.12, 0.7, 0.005)
        .out(-6.4),
    ChipAndRetro: "NES Bass" => chip(WavetableId::NesTriangle, 0.01)
        .semis(A, -12)
        .mono(0.0)
        .out(3.1),
    ChipAndRetro: "Game Boy Wave" => chip(WavetableId::GameBoy, 0.01)
        .pos(A, 0.3)
        .lfo_sync(1, LfoWave::SawDown, NoteDivision::Sixteenth)
        .route(ModSource::Lfo(1), ModDest::OscPosition(A as u8), 0.3)
        .out(-2.7),
    ChipAndRetro: "C64 Arp" => chip(WavetableId::C64, 0.01)
        .pos(A, 0.4)
        .lfo_sync(1, LfoWave::Square, NoteDivision::Sixteenth)
        .stepped(ModSource::Lfo(1), ModDest::LayerPitch(A as u8), 0.125, 1)
        .out(-0.4),
    ChipAndRetro: "SID Bass" => chip(WavetableId::Pulse, 0.02)
        .pos(A, 0.25)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 800.0, 0.6)
        .filter_route(A, FilterRoute::F1)
        .env(1, 0.0, 0.1, 0.0, 0.08)
        .env_to_cut(0.6)
        .out(7.5),
    ChipAndRetro: "Chip Pad" => chip(WavetableId::NesPulse125, 0.4)
        .uni(A, 3, 8.0)
        .osc(B, WavetableId::GameBoy, -20.0)
        .semis(B, -12)
        .amp(0.2, 0.0, 1.0, 0.4)
        .fx(crush(8.0, 22_050.0, 0.3))
        .fx(reverb(0.35, 0.25))
        .out(-12.8),
    ChipAndRetro: "8-bit Hat" => init()
        .off(A).off(B).off(C).off(SUB)
        .noise(0.0, -10.0)
        .no_filter()
        .amp(0.001, 0.04, 0.0, 0.02)
        .route(ModSource::Velocity, ModDest::LayerGain(NOISE as u8), 0.15)
        .route(ModSource::Macro(0), ModDest::LayerGain(NOISE as u8), 0.2)
        .route(ModSource::Macro(1), ModDest::Amp, 0.2)
        .mac(0, "Level").mac(1, "Body")
        .fx(crush(4.0, 11_025.0, 0.6))
        .out(-11.1),
    ChipAndRetro: "Arcade Zap" => chip(WavetableId::Saw, 0.01)
        .amp(0.002, 0.15, 0.0, 0.01)
        .env(1, 0.0, 0.12, 0.0, 0.1)
        .route(ModSource::Envelope(1), ModDest::LayerPitch(A as u8), 0.375)
        .fx(crush(6.0, 16_000.0, 0.4))
        .out(3.0),
    // A *keys* patch and not a bass one: a bell partial an octave up, a
    // slower attack and a harder crush, so it is somewhere of its own rather
    // than "SID Bass with the filter open" — which is what the pairwise test
    // caught it being.
    ChipAndRetro: "Lo-fi Keys" => chip(WavetableId::Bitwave, 0.15)
        .pos(A, 0.2)
        .osc(B, WavetableId::Sine, -13.0)
        .semis(B, 12)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 3_000.0, 0.5)
        .filter_route(A, FilterRoute::F1)
        .filter_route(B, FilterRoute::Bypass)
        .amp(0.012, 1.4, 0.35, 0.35)
        .fx(crush(8.0, 8_000.0, 0.75))
        .fx(chorus(3, 0.4))
        .out(0.8),

    ChipAndRetro: "Duty Sweep" => chip(WavetableId::PwmSweep, 0.02)
        .pos(A, 0.2)
        .lfo(1, LfoWave::Triangle, 0.7)
        .route(ModSource::Lfo(1), ModDest::OscPosition(A as u8), 0.6)
        .route(ModSource::Macro(2), ModDest::LfoRate(1), 0.5)
        .mac(2, "Sweep")
        .out(2.2),
    ChipAndRetro: "Chip Organ" => chip(WavetableId::NesPulse50, 0.02)
        .osc(B, WavetableId::NesPulse50, -19.0)
        .semis(B, 12)
        .osc(C, WavetableId::NesPulse50, -25.0)
        .semis(C, 19)
        .amp(0.002, 0.0, 1.0, 0.02)
        .out(-6.9),
    ChipAndRetro: "Amiga Lead" => chip(WavetableId::Crunch, 0.05)
        .pos(A, 0.35)
        .uni(A, 2, 7.0)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 5_000.0, 0.5)
        .filter_route(A, FilterRoute::F1)
        .amp(0.002, 0.4, 0.6, 0.05)
        .fx(crush(8.0, 22_050.0, 0.8))
        .fx(delay(NoteDivision::SixteenthDotted, 0.35, 0.25))
        .out(-0.6),
    // One bit through a paper cone: no filter to speak of, no envelope, and
    // everything below a kilohertz thrown away — which is the whole sound.
    ChipAndRetro: "PC Speaker" => chip(WavetableId::Square, 0.001)
        .filter(0, FilterModel::Clean, SvfMode::Highpass, 1_200.0, 0.6)
        .filter_route(A, FilterRoute::F1)
        .amp(0.0, 0.0, 1.0, 0.001)
        .mono(0.0)
        .out(10.1),

    // ---------------------------------------------------- Sequence & Arp ---
    // Rhythm from synced LFOs on the amp or the filter, so a held note *is*
    // the sequence — the roll's arpeggiate command is the arpeggiator.
    SequenceAndArp: "Gated Pad" => pad(WavetableId::Saw, 2_500.0, 0.3, 0.8)
        .lfo_sync(1, LfoWave::Square, NoteDivision::Sixteenth)
        .smooth(1, 0.15)
        .route(ModSource::Lfo(1), ModDest::Amp, 1.0)
        .out(-19.1),
    SequenceAndArp: "Trance Pluck" => pluck(WavetableId::Saw, 900.0, 0.35)
        .uni(A, 7, 18.0)
        .width(A, 0.9)
        .fx(ping_pong(NoteDivision::EighthDotted, 0.4, 0.35))
        .fx(reverb(0.6, 0.3))
        .out(7.2),
    SequenceAndArp: "Sidechain Feel" => pad(WavetableId::AnalogMorph, 2_000.0, 0.02, 0.25)
        .pos(A, 0.55)
        .lfo_sync(1, LfoWave::SawUp, NoteDivision::Quarter)
        .route(ModSource::Lfo(1), ModDest::Amp, 0.7)
        .out(22.2),
    SequenceAndArp: "Filter Step" => init()
        .osc(A, WavetableId::Saw, -14.0)
        .uni(A, 3, 10.0)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 600.0, 0.5)
        .amp(0.004, 0.0, 1.0, 0.15)
        .lfo_sync(0, LfoWave::SampleHold, NoteDivision::Eighth)
        .smooth(0, 0.1)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.8)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.2)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.5)
        .route(ModSource::Macro(1), ModDest::FilterResonance(0), 0.5)
        .mac(0, "Step rate").mac(1, "Resonance")
        .out(4.3),
    SequenceAndArp: "Octave Bounce" => init()
        .amp(0.001, 0.12, 0.25, 0.04)
        .osc(A, WavetableId::Saw, -14.0)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 5_000.0, 0.5)
        .amp(0.002, 0.2, 0.4, 0.1)
        .lfo_sync(0, LfoWave::Square, NoteDivision::Eighth)
        .stepped(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.125, 1)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.2)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.5)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.3)
        .mac(0, "Rate").mac(1, "Tone")
        .out(5.0),
    SequenceAndArp: "Pulse Train" => init()
        .osc(A, WavetableId::Square, -14.0)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 4_000.0, 0.6)
        .amp(0.002, 0.0, 1.0, 0.1)
        .lfo_sync(0, LfoWave::Square, NoteDivision::Sixteenth)
        .route(ModSource::Lfo(0), ModDest::Amp, 1.0)
        .lfo_sync(1, LfoWave::Triangle, NoteDivision::Whole)
        .route(ModSource::Lfo(1), ModDest::FilterCutoff(0), 0.5)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.2)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.5)
        .route(ModSource::Macro(1), ModDest::LfoRate(1), 0.5)
        .mac(0, "Gate rate").mac(1, "Sweep rate")
        .out(-22.0),
    SequenceAndArp: "Random Bleeps" => init()
        .osc(A, WavetableId::Sine, -12.0)
        .off(B).off(C).off(SUB)
        .no_filter()
        .amp(0.001, 0.0, 1.0, 0.05)
        .lfo_sync(0, LfoWave::SampleHold, NoteDivision::Sixteenth)
        .stepped(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.25, 12)
        .lfo_sync(1, LfoWave::Square, NoteDivision::Sixteenth)
        .route(ModSource::Lfo(1), ModDest::Amp, 1.0)
        .route(ModSource::Velocity, ModDest::Amp, 0.15)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.5)
        .route(ModSource::Macro(1), ModDest::LfoDepth(0), 0.6)
        .mac(0, "Rate").mac(1, "Range")
        .fx(delay(NoteDivision::Eighth, 0.35, 0.3))
        .out(-29.1),
    SequenceAndArp: "Tremolo Keys" => electric_piano(0.25, 2.0)
        .lfo_sync(1, LfoWave::Triangle, NoteDivision::Eighth)
        .lfo_depth(1, 1.0)
        .route(ModSource::Lfo(1), ModDest::LayerPan(A as u8), 0.4)
        .out(6.9),

    SequenceAndArp: "Bass Sequence" => bass(WavetableId::Saw, 900.0, 0.05)
        .lfo_sync(1, LfoWave::Square, NoteDivision::Sixteenth)
        .smooth(1, 0.05)
        .route(ModSource::Lfo(1), ModDest::Amp, 1.0)
        .lfo_sync(0, LfoWave::SampleHold, NoteDivision::Eighth)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.6)
        .amp(0.002, 0.0, 1.0, 0.05)
        .out(-24.0),
    SequenceAndArp: "Arp Bells" => bell(WavetableId::Glass, 0.5)
        .pos(A, 0.4)
        .lfo_sync(1, LfoWave::Square, NoteDivision::Sixteenth)
        .route(ModSource::Lfo(1), ModDest::Amp, 1.0)
        .lfo_sync(2, LfoWave::SampleHold, NoteDivision::Eighth)
        .stepped(ModSource::Lfo(2), ModDest::LayerPitch(A as u8), 0.25, 7)
        .amp(0.001, 0.5, 0.0, 0.1)
        .fx(ping_pong(NoteDivision::EighthDotted, 0.35, 0.3))
        .fx(reverb(0.6, 0.3))
        .out(-13.3),
    SequenceAndArp: "Sync Seq" => init()
        .osc(A, WavetableId::SyncSweep, -14.0)
        .warp(A, WarpMode::Sync, 0.3)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 7_000.0, 0.5)
        .amp(0.002, 0.0, 1.0, 0.06)
        .lfo_sync(0, LfoWave::SampleHold, NoteDivision::Sixteenth)
        .route(ModSource::Lfo(0), ModDest::OscWarp(A as u8), 0.8)
        .lfo_sync(1, LfoWave::Square, NoteDivision::Sixteenth)
        .route(ModSource::Lfo(1), ModDest::Amp, 1.0)
        .route(ModSource::Velocity, ModDest::OscWarp(A as u8), 0.2)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.5)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.3)
        .mac(0, "Step rate").mac(1, "Tone")
        .fx(delay(NoteDivision::Eighth, 0.3, 0.25))
        .out(-19.7),
    SequenceAndArp: "Noise Rhythm" => init()
        .off(A).off(B).off(C).off(SUB)
        .noise(0.25, -10.0)
        .filter_route(NOISE, FilterRoute::F1)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 2_500.0, 1.0)
        .amp(0.001, 0.0, 1.0, 0.04)
        .lfo_sync(0, LfoWave::Square, NoteDivision::Sixteenth)
        .route(ModSource::Lfo(0), ModDest::Amp, 1.0)
        .lfo_sync(1, LfoWave::SampleHold, NoteDivision::Eighth)
        .route(ModSource::Lfo(1), ModDest::FilterCutoff(0), 0.8)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.2)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.5)
        .route(ModSource::Macro(1), ModDest::FilterResonance(0), 0.4)
        .mac(0, "Rate").mac(1, "Resonance")
        .fx(ping_pong(NoteDivision::Sixteenth, 0.35, 0.3))
        .out(-12.7),
    SequenceAndArp: "Motion Keys" => init()
        .osc(A, WavetableId::Saw, -14.0)
        .uni(A, 2, 6.0)
        .osc(B, WavetableId::Pulse, -20.0)
        .pos(B, 0.35)
        .off(C).off(SUB)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 1_600.0, 0.6)
        .amp(0.004, 2.0, 0.4, 0.3)
        .lfo_sync(0, LfoWave::Triangle, NoteDivision::Quarter)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.5)
        .lfo_sync(1, LfoWave::SawDown, NoteDivision::Eighth)
        .route(ModSource::Lfo(1), ModDest::Amp, 0.45)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.25)
        .route(ModSource::Macro(0), ModDest::LfoRate(1), 0.5)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.3)
        .mac(0, "Pulse rate").mac(1, "Tone")
        .fx(chorus(3, 0.3))
        .out(-6.3),

    // ------------------------------------------------------- Atmos & FX ---
    AtmosAndFx: "Wind" => atmos(0.85, SvfMode::Bandpass, 400.0).out(22.5),
    AtmosAndFx: "Rain" => atmos(0.0, SvfMode::Highpass, 4_000.0)
        .lfo(0, LfoWave::SampleHold, 30.0)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.4)
        .amp(0.5, 0.0, 1.0, 1.0)
        .out(-7.2),
    AtmosAndFx: "Ocean" => atmos(0.95, SvfMode::Lowpass, 700.0)
        .lfo(0, LfoWave::Sine, 0.06)
        .fx(delay(NoteDivision::Half, 0.3, 0.3))
        .out(-1.7),
    AtmosAndFx: "Riser" => init()
        .osc(A, WavetableId::Saw, -16.0)
        .uni(A, 6, 25.0)
        .off(B).off(C).off(SUB)
        .noise(0.2, -30.0)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 400.0, 0.6)
        .amp(0.05, 0.0, 1.0, 0.5)
        .env(1, 4.0, 0.0, 1.0, 1.0)
        .env_to_cut(0.9)
        .route(ModSource::Envelope(1), ModDest::LayerPitch(A as u8), 0.25)
        .route(ModSource::Envelope(1), ModDest::LayerGain(NOISE as u8), 0.3)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.2)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::OscUnisonDetune(A as u8), 0.5)
        .mac(0, "Tone").mac(1, "Spread")
        .fx(reverb(0.8, 0.35))
        .out(4.9),
    AtmosAndFx: "Downer" => init()
        .amp(0.005, 0.0, 1.0, 1.5)
        .osc(A, WavetableId::Saw, -16.0)
        .uni(A, 6, 25.0)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 8_000.0, 0.6)
        .amp(0.05, 0.0, 1.0, 0.5)
        .env(1, 3.0, 0.0, 1.0, 1.0)
        .env_to_cut(-0.9)
        .route(ModSource::Envelope(1), ModDest::LayerPitch(A as u8), -0.25)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.15)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::OscUnisonDetune(A as u8), 0.5)
        .mac(0, "Tone").mac(1, "Spread")
        .fx(reverb(0.8, 0.35))
        .out(3.5),
    AtmosAndFx: "Space Drone" => init()
        .osc(A, WavetableId::Hollow, -17.0)
        .pos(A, 0.7)
        .uni(A, 3, 5.0)
        .osc(B, WavetableId::Sine, -22.0)
        .semis(B, -24)
        .off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 800.0, 0.7)
        .amp(1.2, 0.0, 1.0, 2.0)
        .lfo(0, LfoWave::Sine, 0.03).lfo_mode(0, LfoMode::Free)
        .lfo(1, LfoWave::Sine, 0.05).lfo_mode(1, LfoMode::Free)
        .lfo(2, LfoWave::Sine, 0.07).lfo_mode(2, LfoMode::Free)
        .lfo(3, LfoWave::Sine, 0.11).lfo_mode(3, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.4)
        .route(ModSource::Lfo(1), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Lfo(2), ModDest::LayerPan(A as u8), 0.4)
        .route(ModSource::Lfo(3), ModDest::LayerGain(B as u8), 0.25)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.15)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::LfoRate(0), 0.6)
        .mac(0, "Tone").mac(1, "Drift")
        .fx(reverb(1.0, 0.55))
        .out(3.5),
    AtmosAndFx: "Sci-fi Sweep" => init()
        .osc(A, WavetableId::SyncSweep, -14.0)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 2_000.0, 1.2)
        .amp(0.02, 0.0, 1.0, 0.4)
        .lfo_sync(0, LfoWave::SawUp, NoteDivision::Whole)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 1.0)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.5)
        .route(ModSource::Velocity, ModDest::FilterResonance(0), 0.2)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.5)
        .route(ModSource::Macro(1), ModDest::FilterResonance(0), 0.4)
        .mac(0, "Sweep rate").mac(1, "Resonance")
        .fx(ping_pong(NoteDivision::Eighth, 0.4, 0.3))
        .out(-0.9),
    AtmosAndFx: "Laser" => init()
        .osc(A, WavetableId::Sine, -11.0)
        .off(B).off(C).off(SUB)
        .no_filter()
        .amp(0.002, 0.3, 0.0, 0.05)
        .env(1, 0.0, 0.25, 0.0, 0.2)
        .route(ModSource::Envelope(1), ModDest::LayerPitch(A as u8), 0.5)
        .route(ModSource::Velocity, ModDest::LayerGain(A as u8), 0.15)
        .route(ModSource::Macro(0), ModDest::LayerPitch(A as u8), 0.05)
        .route(ModSource::Macro(1), ModDest::Amp, 0.2)
        .mac(0, "Pitch").mac(1, "Level")
        .fx(delay(NoteDivision::Sixteenth, 0.4, 0.4))
        .out(-10.3),
    AtmosAndFx: "Impact" => init()
        .osc(A, WavetableId::SubSine, -8.0)
        .semis(A, -12)
        .off(B).off(C).off(SUB)
        .noise(0.6, -22.0)
        .no_filter()
        .amp(0.002, 1.5, 0.0, 0.5)
        .env(1, 0.0, 0.08, 0.0, 0.06)
        .route(ModSource::Envelope(1), ModDest::LayerPitch(A as u8), 0.25)
        .env(2, 0.0, 0.2, 0.0, 0.15)
        .route(ModSource::Envelope(2), ModDest::LayerGain(NOISE as u8), 0.3)
        .route(ModSource::Velocity, ModDest::LayerGain(A as u8), 0.15)
        .route(ModSource::Macro(0), ModDest::LayerPitch(A as u8), 0.05)
        .route(ModSource::Macro(1), ModDest::LayerGain(NOISE as u8), 0.25)
        .mac(0, "Tune").mac(1, "Noise")
        .fx(reverb(0.9, 0.45))
        .out(-15.7),
    AtmosAndFx: "Glitch" => init()
        .osc(A, WavetableId::Bitwave, -13.0)
        .pos(A, 0.5)
        .off(B).off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 6_000.0, 0.7)
        .amp(0.001, 0.0, 1.0, 0.08)
        .lfo_sync(0, LfoWave::SampleHold, NoteDivision::ThirtySecond)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 1.0)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.8)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.5)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.2)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.5)
        .route(ModSource::Macro(1), ModDest::FilterResonance(0), 0.4)
        .mac(0, "Rate").mac(1, "Resonance")
        .fx(crush(5.0, 6_000.0, 0.5))
        .out(7.0),

    AtmosAndFx: "Sonar" => init()
        .osc(A, WavetableId::Sine, -11.0)
        .semis(A, 12)
        .off(B).off(C).off(SUB)
        .no_filter()
        .amp(0.004, 0.35, 0.0, 0.2)
        .env(1, 0.0, 0.3, 0.0, 0.2)
        .route(ModSource::Envelope(1), ModDest::LayerPitch(A as u8), -0.02)
        .route(ModSource::Velocity, ModDest::LayerGain(A as u8), 0.15)
        .route(ModSource::Macro(0), ModDest::LayerPitch(A as u8), 0.05)
        .route(ModSource::Macro(1), ModDest::Amp, 0.2)
        .mac(0, "Pitch").mac(1, "Level")
        .fx(ping_pong(NoteDivision::Half, 0.55, 0.45))
        .fx(reverb(0.95, 0.5))
        .out(-11.2),
    AtmosAndFx: "Metal Scrape" => atmos(0.1, SvfMode::Bandpass, 2_500.0)
        .osc(A, WavetableId::Gong, -14.0)
        .pos(A, 0.6)
        .warp(A, WarpMode::Rm, 0.8)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 11)
        .filter_route(A, FilterRoute::F1)
        .amp(0.4, 0.0, 1.0, 1.2)
        .out(21.5),
    AtmosAndFx: "Sub Drop" => init()
        .osc(A, WavetableId::SubSine, -8.0)
        .off(B).off(C).off(SUB)
        .no_filter()
        .amp(0.005, 0.0, 1.0, 0.6)
        .env(1, 0.0, 2.5, 0.0, 1.0)
        .route(ModSource::Envelope(1), ModDest::LayerPitch(A as u8), 0.35)
        .route(ModSource::Velocity, ModDest::LayerGain(A as u8), 0.15)
        .route(ModSource::Macro(0), ModDest::LayerPitch(A as u8), 0.05)
        .route(ModSource::Macro(1), ModDest::Amp, 0.2)
        .mac(0, "Tune").mac(1, "Level")
        .out(-17.5),
    // The reversed cymbal: an attack long enough to be the whole sound, and a
    // release short enough to be a cut.
    AtmosAndFx: "Reverse Swell" => init()
        .off(A).off(B).off(C).off(SUB)
        .noise(0.15, -8.0)
        .filter_route(NOISE, FilterRoute::F1)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 1_200.0, 0.8)
        .amp(2.4, 0.0, 1.0, 0.02)
        .env(1, 2.4, 0.0, 1.0, 0.02)
        .env_to_cut(0.85)
        .route(ModSource::Envelope(1), ModDest::FilterResonance(0), 0.25)
        .route(ModSource::Velocity, ModDest::LayerGain(NOISE as u8), 0.15)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::FilterResonance(0), 0.4)
        .mac(0, "Tone").mac(1, "Focus")
        .fx(reverb(0.85, 0.45))
        .out(2.3),
    AtmosAndFx: "Static" => init()
        .off(A).off(B).off(C).off(SUB)
        .noise(0.35, -10.0)
        .filter_route(NOISE, FilterRoute::F1)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 1_800.0, 1.4)
        .amp(0.02, 0.0, 1.0, 0.08)
        // Fast enough that a tenth of a second always contains some of it: at
        // eighteen hertz and this depth the gaps were longer than the
        // measurement window, and the preset read as silent.
        .lfo(0, LfoWave::SampleHold, 34.0)
        .lfo_mode(0, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.4)
        .lfo(1, LfoWave::SampleHold, 3.0)
        .lfo_mode(1, LfoMode::Free)
        .route(ModSource::Lfo(1), ModDest::FilterCutoff(0), 0.5)
        .route(ModSource::Velocity, ModDest::LayerGain(NOISE as u8), 0.15)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.6)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.4)
        .mac(0, "Crackle").mac(1, "Tune")
        .fx(crush(5.0, 8_000.0, 0.6))
        .out(-10.6),

    // --------------------------------------------------------- Synth Drums ---
    // The drum machine is the drum instrument; these exist because a synth
    // kick or a zap is a thing a person reaches for in a synth, and they live
    // on a key rather than a map.
    SynthDrums: "808 Kick" => drum(WavetableId::SubSine, 30.0, 0.045, 0.7)
        .semis(A, -24)
        .fx(drive_fx(DistortionCurve::SoftClip, 6.0, 0.2))
        .out(-14.2),
    SynthDrums: "909 Kick" => drum(WavetableId::Sine, 36.0, 0.06, 0.35)
        .semis(A, -24)
        .noise(0.0, -28.0)
        .env(2, 0.0, 0.015, 0.0, 0.01)
        .route(ModSource::Envelope(2), ModDest::LayerGain(NOISE as u8), 0.25)
        .fx(drive_fx(DistortionCurve::SoftClip, 10.0, 0.3))
        .out(-13.1),
    SynthDrums: "Snare Synth" => drum(WavetableId::Sine, 12.0, 0.03, 0.18)
        .semis(A, -12)
        .noise(0.0, -12.0)
        .filter_route(NOISE, FilterRoute::F1)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 3_000.0, 0.8)
        .out(11.7),
    SynthDrums: "Hat Synth" => init()
        .off(A).off(B).off(C).off(SUB)
        .noise(0.0, -10.0)
        .filter_route(NOISE, FilterRoute::F1)
        .filter(0, FilterModel::Clean, SvfMode::Highpass, 7_000.0, 0.8)
        .amp(0.001, 0.06, 0.0, 0.03)
        .route(ModSource::Velocity, ModDest::LayerGain(NOISE as u8), 0.15)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::Amp, 0.2)
        .mac(0, "Tone").mac(1, "Level")
        .out(-11.0),
    SynthDrums: "Tom Synth" => drum(WavetableId::Sine, 18.0, 0.12, 0.4)
        .semis(A, -12)
        .out(-12.0),
    SynthDrums: "Zap" => drum(WavetableId::Saw, 48.0, 0.09, 0.12)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 3_000.0, 0.6)
        .filter_route(A, FilterRoute::F1)
        .env(2, 0.0, 0.09, 0.0, 0.06)
        .route(ModSource::Envelope(2), ModDest::FilterCutoff(0), -0.5)
        .out(-2.3),
    SynthDrums: "Rim Synth" => drum(WavetableId::Square, 20.0, 0.008, 0.05)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 1_800.0, 1.4)
        .filter_route(A, FilterRoute::F1)
        .amp(0.000_5, 0.05, 0.0, 0.02)
        .out(-1.6),
    // Four bursts and not one: the square LFO on the amp is the four hands,
    // and it is why this is a clap rather than a short snare.
    SynthDrums: "Clap Synth" => init()
        .off(A).off(B).off(C).off(SUB)
        .noise(0.1, -10.0)
        .filter_route(NOISE, FilterRoute::F1)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 1_400.0, 0.9)
        .amp(0.002, 0.22, 0.0, 0.08)
        .lfo(0, LfoWave::Square, 55.0)
        .lfo_mode(0, LfoMode::Free)
        .env(1, 0.0, 0.035, 0.0, 0.02)
        .route(ModSource::Envelope(1), ModDest::Amp, 0.5)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.3)
        .route(ModSource::Velocity, ModDest::LayerGain(NOISE as u8), 0.15)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::Amp, 0.2)
        .mac(0, "Tone").mac(1, "Level")
        .fx(reverb(0.35, 0.25))
        .out(-4.0),
    SynthDrums: "Cowbell" => init()
        .osc(A, WavetableId::Square, -12.0)
        .semis(A, 7)
        .osc(B, WavetableId::Square, -14.0)
        .semis(B, 18)
        .fine(B, 40.0)
        .off(C).off(SUB)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 2_600.0, 0.9)
        .amp(0.001, 0.28, 0.0, 0.08)
        .route(ModSource::Velocity, ModDest::LayerGain(A as u8), 0.12)
        .route(ModSource::Macro(0), ModDest::LayerPitch(A as u8), 0.02)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.3)
        .mac(0, "Tune").mac(1, "Tone")
        .out(2.5),
    SynthDrums: "Conga Synth" => drum(WavetableId::Sine, 8.0, 0.05, 0.3)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 400.0, 0.8)
        .filter_route(A, FilterRoute::F1)
        .amp(0.001, 0.3, 0.0, 0.1)
        .out(-8.7),
    SynthDrums: "Open Hat" => init()
        .off(A).off(B).off(C).off(SUB)
        .noise(0.0, -10.0)
        .filter_route(NOISE, FilterRoute::F1)
        .filter(0, FilterModel::Clean, SvfMode::Highpass, 6_000.0, 0.7)
        .amp(0.001, 0.55, 0.0, 0.2)
        .route(ModSource::Velocity, ModDest::LayerGain(NOISE as u8), 0.15)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(1), ModDest::Amp, 0.2)
        .mac(0, "Tone").mac(1, "Level")
        .out(-13.6),
    // ================================================================
    // The electronic expansion (2026-09-09)
    //
    // > *"right now it's very general but i want more presets that utilize
    // > its advanced synth capabilities to make some really cool unique
    // > electronic sounds ... much more cool stuff to show off just
    // > immediately out of the box."*
    //
    // The bank above is an *instrument* bank: it answers "what does a
    // trombone sound like". These answer "what can this synth do that a
    // sampler cannot" — hard sync, through-zero FM, ring modulation, phase
    // quantising, wavetable morphs driven by anything, unison that opens and
    // closes, and the sources a score can play *into* a note. Measured, the
    // bank used about three fifths of the engine and had never once reached
    // for `Quantise`, `Random`, `NoteOnCounter`, `Aftertouch`, the roll's own
    // per-note X/Y, or three of the forty tables. `tests/flopsynth_shows_off.rs`
    // is what keeps it that way.
    // ================================================================

    // ------------------------------------------------------------- Bass ---
    // Bass music is where a wavetable synth earns its keep: the sound is the
    // *movement*, so almost every one of these has something walking.
    BassMusic: "Neuro Morph" => bass(WavetableId::Growl, 900.0, 0.15)
        .pos(A, 0.15)
        .uni(A, 2, 8.0)
        .osc(B, WavetableId::Reese, -14.0)
        .semis(B, -12)
        .lfo_sync(0, LfoWave::Triangle, NoteDivision::Sixteenth)
        .lfo_mode(0, LfoMode::Retrigger)
        // The morph *is* the sound: the table walked under a synced LFO is
        // what a neuro bass does that a filter sweep cannot.
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.85)
        .route(ModSource::Lfo(0), ModDest::FilterDrive(0), 0.5)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.6)
        .route(ModSource::Macro(1), ModDest::OscPosition(A as u8), 0.4)
        .mac(0, "Rate").mac(1, "Morph")
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 1_400.0, 0.55)
        .fx(drive_fx(DistortionCurve::Diode, 12.0, 0.45))
        .out(1.9),
    BassMusic: "Talk Bass" => bass(WavetableId::Vowel, 1_100.0, 0.09)
        .pos(A, 0.2)
        .osc(B, WavetableId::SubSine, -10.0)
        .semis(B, -12)
        .lfo_sync(0, LfoWave::SampleHold, NoteDivision::Sixteenth)
        .lfo_mode(0, LfoMode::Free)
        .amp(0.002, 0.18, 0.0, 0.08)
        // Walking a vowel table is a formant sweep, which the ear hears as a
        // mouth rather than as a filter.
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.9)
        .route(ModSource::ModWheel, ModDest::OscPosition(A as u8), 0.5)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 700.0, 0.5)
        .character(0, 0.5)
        .mac(0, "Vowel")
        .route(ModSource::Macro(0), ModDest::FilterCharacter(0), 0.8)
        .out(2.5),
    BassMusic: "Sync Bass" => bass(WavetableId::Saw, 6_500.0, 0.05)
        .warp(A, WarpMode::Sync, 0.3)
        .env(2, 0.0, 0.35, 0.0, 0.1)
        // Hard sync swept by an envelope: the classic tearing bass, and the
        // one thing `OscWarp` as a destination is *for*.
        .route(ModSource::Envelope(2), ModDest::OscWarp(A as u8), 0.7)
        .route(ModSource::Velocity, ModDest::OscWarp(A as u8), 0.3)
        .osc(B, WavetableId::SubSine, -9.0)
        .semis(B, -12)
        .out(-6.3),
    BassMusic: "Bitcrush Bass" => bass(WavetableId::Square, 1_100.0, 0.34)
        // `Quantise` holds the read to a handful of steps a cycle — the
        // synth's own digital grit, and nothing in the bank had used it.
        .warp(A, WarpMode::Quantise, 0.55)
        .route(ModSource::Envelope(1), ModDest::OscWarp(A as u8), 0.4)
        .osc(B, WavetableId::SubSquare, -12.0)
        .semis(B, -12)
        .lfo_sync(0, LfoWave::Square, NoteDivision::Sixteenth)
        .lfo_mode(0, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.7)
        .smooth(0, 0.08)
        .amp(0.002, 0.0, 1.0, 0.2)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 1_100.0, 0.4)
        .out(-21.9),
    BassMusic: "Drift Bass" => bass(WavetableId::Sawstack, 620.0, 0.62)
        .uni(A, 3, 6.0)
        // A different tuning every note, by a few cents — what an analogue
        // bank does because it cannot help it, and what a digital one has to
        // be *asked* for. `Random` is per note.
        .route(ModSource::Random, ModDest::LayerPitch(A as u8), 0.012)
        .route(ModSource::Random, ModDest::FilterCutoff(0), 0.12)
        .route(ModSource::Random, ModDest::UnisonDetune, 0.25)
        .lfo(0, LfoWave::Triangle, 0.13)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.55)
        .amp(0.02, 0.0, 1.0, 0.6)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 620.0, 0.5)
        .out(2.2),
    BassMusic: "Hoover Bass" => bass(WavetableId::Hoover, 9_500.0, 0.55)
        .uni(A, 3, 18.0)
        .osc(B, WavetableId::PwmSweep, -13.0)
        .semis(B, -12)
        .uni(B, 3, 22.0)
        .lfo(0, LfoWave::Triangle, 0.7)
        .route(ModSource::Lfo(0), ModDest::OscPosition(B as u8), 0.6)
        .env(2, 0.0, 0.45, 0.0, 0.1)
        .route(ModSource::Envelope(2), ModDest::LayerPitch(A as u8), -0.09)
        .amp(0.004, 0.0, 1.0, 0.25)
        .fx(chorus(3, 0.45))
        .out(-0.5),
    BassMusic: "Wide Sub" => bass(WavetableId::Wide, 420.0, 0.85)
        .uni(A, 2, 4.0)
        .width(A, 1.0)
        .osc(B, WavetableId::SubSquare, -11.0)
        .semis(B, -12)
        .route(ModSource::Envelope(1), ModDest::OscUnisonBlend(A as u8), 0.5)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 900.0, 0.3)
        .out(-4.6),
    BassMusic: "Stairs Bass" => bass(WavetableId::Stairs, 820.0, 1.1)
        .pos(A, 0.4)
        .lfo_sync(0, LfoWave::Square, NoteDivision::Eighth)
        .lfo_mode(0, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.5)
        .smooth(0, 0.2)
        .warp(A, WarpMode::Bend, 0.4)
        .route(ModSource::Velocity, ModDest::OscWarp(A as u8), 0.4)
        .osc(B, WavetableId::SubTri, -10.0)
        .semis(B, -12)
        .out(-17.2),
    BassMusic: "Triplet Wobble" => bass(WavetableId::Growl, 400.0, 0.2)
        .pos(A, 0.35)
        .lfo_sync(0, LfoWave::Sine, NoteDivision::EighthTriplet)
        .lfo_mode(0, LfoMode::Retrigger)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.85)
        // The wheel sets how far the wobble swings rather than adding one of
        // its own — a route whose *depth* is played, which is what `via` is.
        .route_via(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.6, ModSource::ModWheel)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 320.0, 0.6)
        .fx(drive_fx(DistortionCurve::SoftClip, 8.0, 0.3))
        .out(1.8),
    BassMusic: "Crunch Bass" => bass(WavetableId::Crunch, 9_000.0, 0.07)
        .pos(A, 0.5)
        .route(ModSource::Envelope(1), ModDest::FilterDrive(0), 0.35)
        .drive(0, 0.2)
        .amp(0.001, 0.14, 0.0, 0.06)
        .env(1, 0.0, 0.09, 0.0, 0.05)
        .env_to_cut(0.9)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 2_600.0, 0.5)
        .osc(B, WavetableId::SubSine, -10.0)
        .semis(B, -12)
        .out(-3.6),

    // ------------------------------------------------------------- Lead ---
    SyncAndFm: "Sync Scream" => lead(WavetableId::Saw, 6_000.0, 0.0)
        .warp(A, WarpMode::Sync, 0.25)
        .env(2, 0.0, 0.9, 0.15, 0.2)
        .route(ModSource::Envelope(2), ModDest::OscWarp(A as u8), 0.75)
        .route(ModSource::ModWheel, ModDest::OscWarp(A as u8), 0.5)
        .uni(A, 2, 7.0)
        .fx(delay(NoteDivision::EighthDotted, 0.35, 0.22))
        .out(7.1),
    SyncAndFm: "FM Stack" => lead(WavetableId::Sine, 9_000.0, 0.0)
        .warp(A, WarpMode::Fm, 0.45)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 12)
        .env(2, 0.002, 0.5, 0.2, 0.2)
        // The index falling is what makes an FM note *struck* rather than
        // droning — the whole of a DX brass.
        .route(ModSource::Envelope(2), ModDest::OscWarp(A as u8), 0.6)
        .route(ModSource::Velocity, ModDest::OscWarp(A as u8), 0.35)
        .route(ModSource::Macro(0), ModDest::LayerPitch(B as u8), 0.06)
        .mac(0, "Ratio")
        .out(1.2),
    Expressive: "Formant Lead" => lead(WavetableId::FormantSweep, 7_000.0, 0.02)
        .lfo(0, LfoWave::Triangle, 0.9)
        .late(0, 0.25, 0.4)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.7)
        .route(ModSource::Aftertouch, ModDest::LfoDepth(0), 0.8)
        .uni(A, 2, 5.0)
        .fx(reverb(0.5, 0.2))
        .out(17.2),
    Expressive: "Detune Monster" => lead(WavetableId::Sawstack, 8_000.0, 0.0)
        .uni(A, 7, 22.0)
        .width(A, 1.0)
        .blend(A, 0.8)
        .env(2, 0.6, 0.0, 1.0, 0.4)
        // Unison that *opens*: the sides walk out from the centre as the note
        // is held, which no static detune can do.
        .route(ModSource::Envelope(2), ModDest::OscUnisonDetune(A as u8), 0.6)
        .route(ModSource::Envelope(2), ModDest::OscUnisonBlend(A as u8), 0.5)
        .route(ModSource::ModWheel, ModDest::UnisonDetune, 0.4)
        .fx(reverb(0.6, 0.22))
        .out(16.3),
    SyncAndFm: "Grit Lead" => lead(WavetableId::Bitwave, 6_000.0, 0.0)
        .warp(A, WarpMode::Quantise, 0.4)
        .lfo_sync(0, LfoWave::SampleHold, NoteDivision::Sixteenth)
        .route(ModSource::Lfo(0), ModDest::OscWarp(A as u8), 0.45)
        .fx(crush(9.0, 14_000.0, 0.3))
        .out(14.8),
    Expressive: "Touch Lead" => lead(WavetableId::Pulse, 4_500.0, 0.03)
        .pos(A, 0.35)
        // Aftertouch is the one gesture a keyboard has that a piano roll does
        // not, and nothing in the bank had read it.
        .route(ModSource::Aftertouch, ModDest::FilterCutoff(0), 0.6)
        .route(ModSource::Aftertouch, ModDest::OscPosition(A as u8), 0.4)
        .lfo(0, LfoWave::Sine, 5.5)
        .route_via(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.006, ModSource::Aftertouch)
        .mono(0.04)
        .out(14.2),
    SyncAndFm: "Bend Lead" => lead(WavetableId::Odd, 2_800.0, 0.0)
        .warp(A, WarpMode::Bend, 0.85)
        .amp(0.001, 0.16, 0.0, 0.07)
        .env(1, 0.0, 0.1, 0.0, 0.06)
        .env_to_cut(0.85)
        .route(ModSource::PitchBend, ModDest::OscWarp(A as u8), 0.5)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.3)
        .out(8.2),
    SyncAndFm: "Mirror Lead" => lead(WavetableId::Square, 6_500.0, 0.0)
        .warp(A, WarpMode::Mirror, 0.6)
        .lfo(0, LfoWave::Sine, 0.4)
        .route(ModSource::Lfo(0), ModDest::OscWarp(A as u8), 0.5)
        .uni(A, 2, 6.0)
        .fx(chorus(2, 0.25))
        .out(1.9),
    Expressive: "Per-Note Morph" => lead(WavetableId::AnalogMorph, 12_000.0, 0.04)
        .lfo_sync(0, LfoWave::Square, NoteDivision::Sixteenth)
        .lfo_mode(0, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.6)
        .smooth(0, 0.06)
        .uni(A, 3, 11.0)
        .route_via(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.5, ModSource::NoteModY)
        // The roll's own per-note X and Y (§16.5) — a note that carries its
        // own timbre, drawn on the note rather than automated on the track.
        .route(ModSource::NoteModX, ModDest::OscPosition(A as u8), 0.9)
        .route(ModSource::NoteModY, ModDest::FilterCutoff(0), 0.7)
        .uni(A, 2, 5.0)
        .out(-6.7),
    SyncAndFm: "Even Lead" => lead(WavetableId::Even, 4_200.0, 0.0)
        .osc(B, WavetableId::Odd, -28.0)
        .semis(B, 12)
        .fine(B, 6.0)
        .uni(A, 3, 9.0)
        .lfo(0, LfoWave::Triangle, 0.3)
        .route(ModSource::Lfo(0), ModDest::LayerGain(B as u8), 0.4)
        .fx(delay(NoteDivision::Eighth, 0.3, 0.18))
        .out(2.1),

    // -------------------------------------------------------------- Pad ---
    MotionAndMorph: "Morph Field" => pad(WavetableId::AnalogMorph, 3_600.0, 1.2, 3.0)
        .uni(A, 4, 12.0)
        .lfo(0, LfoWave::Triangle, 0.06)
        .lfo(1, LfoWave::Sine, 0.11)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.8)
        .route(ModSource::Lfo(1), ModDest::FilterCutoff(0), 0.35)
        // Two slow LFOs at frequencies that never line up is how a pad stays
        // interesting for longer than anybody holds a chord.
        .route(ModSource::Lfo(1), ModDest::LfoPhase(0), 0.3)
        .fx(reverb(0.9, 0.4))
        .out(10.3),
    MotionAndMorph: "Counter Bloom" => pad(WavetableId::Choir, 3_000.0, 1.5, 3.5)
        .uni(A, 3, 9.0)
        // `NoteOnCounter` walks with every note played, so a held chord is
        // four different timbres and the next chord is four more.
        .route(ModSource::NoteOnCounter, ModDest::OscPosition(A as u8), 0.6)
        .route(ModSource::NoteOnCounter, ModDest::LayerPan(A as u8), 0.5)
        .fx(reverb(0.95, 0.45))
        .out(16.2),
    Expressive: "Drift Choir" => pad(WavetableId::Vowel, 2_600.0, 1.8, 4.0)
        .uni(A, 4, 7.0)
        .route(ModSource::Random, ModDest::OscPosition(A as u8), 0.35)
        .route(ModSource::Random, ModDest::LayerPan(A as u8), 0.6)
        .route(ModSource::Random, ModDest::LayerPitch(A as u8), 0.008)
        .fx(ensemble(4, 0.35))
        .fx(reverb(0.9, 0.4))
        .out(16.7),
    MotionAndMorph: "Unison Bloom" => pad(WavetableId::Sawstack, 3_200.0, 2.0, 4.0)
        .uni(A, 5, 4.0)
        .env(2, 3.0, 0.0, 1.0, 2.0)
        .route(ModSource::Envelope(2), ModDest::OscUnisonDetune(A as u8), 0.8)
        .route(ModSource::Envelope(2), ModDest::OscUnisonBlend(A as u8), 0.6)
        .fx(reverb(0.95, 0.45))
        .out(16.5),
    Expressive: "Stage Pad" => pad(WavetableId::Hollow, 2_800.0, 1.0, 3.0)
        .uni(A, 3, 8.0)
        // A velocity that changes the envelope's *shape* rather than its
        // level: hit it hard and it swells, brush it and it sits.
        .route(ModSource::Velocity, ModDest::EnvelopeStageLevel(0, 3), 0.5)
        .route(ModSource::Velocity, ModDest::EnvelopeStageTime(0, 1), 0.4)
        .fx(reverb(0.9, 0.4))
        .out(18.8),
    MotionAndMorph: "Sync Drift" => pad(WavetableId::SyncSweep, 6_000.0, 0.25, 1.1)
        .warp(A, WarpMode::Sync, 0.2)
        .lfo(0, LfoWave::Triangle, 0.08)
        .route(ModSource::Lfo(0), ModDest::OscWarp(A as u8), 0.5)
        .uni(A, 3, 10.0)
        .fx(reverb(0.9, 0.42))
        .out(2.9),
    Expressive: "Wide Field" => pad(WavetableId::Wide, 3_000.0, 1.6, 3.6)
        .uni(A, 4, 14.0)
        .width(A, 1.0)
        .blend(A, 0.9)
        .fx(ensemble(4, 0.4))
        .fx(reverb(0.95, 0.45))
        .out(17.7),
    SyncAndFm: "Ring Field" => pad(WavetableId::Glass, 3_200.0, 1.2, 3.0)
        .warp(A, WarpMode::Rm, 0.4)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 7)
        .lfo(0, LfoWave::Sine, 0.09)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(B as u8), 0.02)
        .fx(reverb(0.95, 0.45))
        .out(13.5),

    // ------------------------------------------------ Sequence & Arp ---
    // Everything here is locked to the transport, so a preset dropped on a
    // bar is already in time — which is the difference between a sound and
    // a part.
    MotionAndMorph: "Step Morph" => init()
        .osc(A, WavetableId::AnalogMorph, -12.0)
        .off(B).off(C).off(SUB)
        .lfo_sync(0, LfoWave::SampleHold, NoteDivision::Sixteenth)
        .lfo_mode(0, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.9)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.5)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 2_400.0, 0.5)
        .amp(0.004, 0.0, 1.0, 0.15)
        .fx(delay(NoteDivision::Sixteenth, 0.25, 0.2))
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.35)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.6)
        .route(ModSource::Macro(1), ModDest::OscPosition(A as u8), 0.5)
        .mac(0, "Rate").mac(1, "Morph")
        .out(8.3),
    MotionAndMorph: "Interval Jump" => init()
        .osc(A, WavetableId::Square, -13.0)
        .off(B).off(C).off(SUB)
        .lfo_sync(0, LfoWave::SampleHold, NoteDivision::Eighth)
        .lfo_mode(0, LfoMode::Free)
        // A *quantised* route: the pitch lands on whole semitones, so a
        // random source becomes an arpeggio instead of a siren.
        .stepped(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.12, 5)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 3_000.0, 0.4)
        .amp(0.002, 0.12, 0.0, 0.1)
        .fx(ping_pong(NoteDivision::EighthDotted, 0.35, 0.28))
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.4)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.6)
        .route(ModSource::Macro(1), ModDest::FilterResonance(0), 0.5)
        .mac(0, "Rate").mac(1, "Bite")
        .out(9.2),
    MotionAndMorph: "Counter Steps" => init()
        .osc(A, WavetableId::Pulse, -13.0)
        .off(B).off(C).off(SUB)
        // Every note played moves the sequence on one — a line that never
        // repeats the same way twice without a single automation point.
        .stepped(ModSource::NoteOnCounter, ModDest::LayerPitch(A as u8), 0.1, 4)
        .route(ModSource::NoteOnCounter, ModDest::OscPosition(A as u8), 0.5)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 2_800.0, 0.45)
        .amp(0.002, 0.14, 0.0, 0.1)
        .fx(delay(NoteDivision::Eighth, 0.3, 0.22))
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.4)
        .route(ModSource::Macro(0), ModDest::OscPosition(A as u8), 0.6)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.4)
        .mac(0, "Shape").mac(1, "Tone")
        .out(21.5),
    MotionAndMorph: "Gate Triplet" => init()
        .osc(A, WavetableId::Sawstack, -13.0)
        .uni(A, 3, 10.0)
        .off(B).off(C).off(SUB)
        .lfo_sync(0, LfoWave::Square, NoteDivision::SixteenthTriplet)
        .lfo_mode(0, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.9)
        .smooth(0, 0.15)
        .amp(0.01, 0.0, 1.0, 0.3)
        .fx(reverb(0.7, 0.3))
        .route(ModSource::Velocity, ModDest::Amp, 0.3)
        .route(ModSource::Macro(0), ModDest::LfoDepth(0), 0.7)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.5)
        .mac(0, "Gate").mac(1, "Tone")
        .out(-17.6),
    MotionAndMorph: "Phase Weave" => init()
        .osc(A, WavetableId::Glass, -10.0)
        .uni(A, 2, 6.0)
        .off(B).off(C).off(SUB)
        .lfo_sync(0, LfoWave::Sine, NoteDivision::Quarter)
        .lfo_sync(1, LfoWave::Sine, NoteDivision::QuarterTriplet)
        .route(ModSource::Lfo(0), ModDest::LayerPan(A as u8), 0.7)
        .route(ModSource::Lfo(1), ModDest::FilterCutoff(0), 0.45)
        // Pushing one LFO's phase with the other is how two synced shapes
        // stop agreeing with each other.
        .route(ModSource::Lfo(1), ModDest::LfoPhase(0), 0.5)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 4_000.0, 0.35)
        .amp(0.05, 0.0, 1.0, 0.6)
        .fx(ping_pong(NoteDivision::Quarter, 0.35, 0.28))
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.4)
        .route(ModSource::Macro(0), ModDest::LfoPhase(0), 0.6)
        .route(ModSource::Macro(1), ModDest::FilterResonance(0), 0.5)
        .mac(0, "Weave").mac(1, "Focus")
        .out(6.9),
    MotionAndMorph: "Rate Ramp" => init()
        .osc(A, WavetableId::Bitwave, -13.0)
        .off(B).off(C).off(SUB)
        .lfo(0, LfoWave::Square, 4.0)
        .env(2, 1.2, 0.0, 1.0, 0.3)
        // The gate speeds up as the note is held — a build in one note.
        .route(ModSource::Envelope(2), ModDest::LfoRate(0), 0.85)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.85)
        .amp(0.01, 0.0, 1.0, 0.25)
        .fx(delay(NoteDivision::Sixteenth, 0.3, 0.2))
        .route(ModSource::Velocity, ModDest::LfoRate(0), 0.4)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.7)
        .route(ModSource::Macro(1), ModDest::LfoDepth(0), 0.6)
        .mac(0, "Speed").mac(1, "Depth")
        .out(-10.4),
    MotionAndMorph: "Wheel Gate" => init()
        .osc(A, WavetableId::Saw, -13.0)
        .uni(A, 2, 8.0)
        .off(B).off(C).off(SUB)
        .lfo_sync(0, LfoWave::Square, NoteDivision::Sixteenth)
        .lfo_mode(0, LfoMode::Free)
        // The gate is only there when the wheel is up: `via` makes the wheel
        // the *amount* of a route rather than a second one.
        .route_via(ModSource::Lfo(0), ModDest::Amp, 0.9, ModSource::ModWheel)
        .smooth(0, 0.1)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 3_200.0, 0.4)
        .amp(0.01, 0.0, 1.0, 0.3)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.4)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.6)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.5)
        .mac(0, "Rate").mac(1, "Tone")
        .out(6.7),
    MotionAndMorph: "Drift Pulse" => init()
        .osc(A, WavetableId::NesPulse25, -13.0)
        .off(B).off(C).off(SUB)
        .lfo_sync(0, LfoWave::SawDown, NoteDivision::Sixteenth)
        .lfo_mode(0, LfoMode::Retrigger)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.7)
        .route(ModSource::Random, ModDest::LayerPan(A as u8), 0.7)
        .route(ModSource::Random, ModDest::OscPosition(A as u8), 0.3)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 2_600.0, 0.5)
        .amp(0.002, 0.1, 0.0, 0.08)
        .fx(ping_pong(NoteDivision::Sixteenth, 0.3, 0.25))
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.45)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.6)
        .route(ModSource::Macro(1), ModDest::OscPosition(A as u8), 0.5)
        .mac(0, "Rate").mac(1, "Shape")
        .out(8.7),

    // ------------------------------------------------------ Atmos & FX ---
    MotionAndMorph: "Morph Drone" => atmos(0.3, SvfMode::Lowpass, 1_600.0)
        .osc(A, WavetableId::FormantSweep, -14.0)
        .lfo(0, LfoWave::Triangle, 0.04)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.95)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.4)
        .fx(reverb(1.0, 0.55))
        .out(138.4),
    SyncAndFm: "Sync Riser" => atmos(0.2, SvfMode::Bandpass, 2_000.0)
        .osc(A, WavetableId::SyncSweep, -14.0)
        .warp(A, WarpMode::Sync, 0.1)
        .env(2, 4.0, 0.0, 1.0, 1.0)
        .route(ModSource::Envelope(2), ModDest::OscWarp(A as u8), 0.9)
        .route(ModSource::Envelope(2), ModDest::LayerPitch(A as u8), 0.08)
        .fx(reverb(0.9, 0.45))
        .out(24.2),
    MotionAndMorph: "Grain Cloud" => atmos(0.5, SvfMode::Bandpass, 1_400.0)
        .osc(A, WavetableId::Grit, -14.0)
        .warp(A, WarpMode::Quantise, 0.7)
        .lfo(0, LfoWave::SampleHold, 9.0)
        .route(ModSource::Lfo(0), ModDest::OscWarp(A as u8), 0.5)
        .route(ModSource::Lfo(0), ModDest::LayerPan(A as u8), 0.8)
        .fx(reverb(1.0, 0.6))
        .out(26.7),
    SyncAndFm: "Ring Bell Drone" => atmos(0.3, SvfMode::Lowpass, 3_000.0)
        .osc(A, WavetableId::Gong, -15.0)
        .warp(A, WarpMode::Rm, 0.55)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 6)
        .lfo(0, LfoWave::Sine, 0.05)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(B as u8), 0.03)
        .fx(reverb(1.0, 0.6))
        .out(7.1),
    Expressive: "Aftertouch Swell" => atmos(0.25, SvfMode::Highpass, 500.0)
        .osc(A, WavetableId::Choir, -14.0)
        .uni(A, 5, 24.0)
        .route(ModSource::Aftertouch, ModDest::FilterCutoff(0), 0.8)
        .route(ModSource::Aftertouch, ModDest::Amp, 0.4)
        .route(ModSource::Aftertouch, ModDest::OscUnisonDetune(A as u8), 0.6)
        .fx(reverb(1.0, 0.55))
        .out(-2.3),
    Expressive: "Bend Warp" => atmos(0.3, SvfMode::Bell, 700.0)
        .osc(A, WavetableId::Hollow, -14.0)
        .warp(A, WarpMode::Bend, 0.75)
        .filter(0, FilterModel::Comb, SvfMode::Bandpass, 500.0, 0.6)
        .character(0, 0.8)
        .route(ModSource::PitchBend, ModDest::OscWarp(A as u8), 0.8)
        .route(ModSource::PitchBend, ModDest::FilterCutoff(0), 0.5)
        .fx(reverb(0.95, 0.5))
        .out(-3.9),
    MotionAndMorph: "Mirror Wash" => atmos(0.35, SvfMode::Lowpass, 2_600.0)
        .osc(A, WavetableId::Tine, -14.0)
        .warp(A, WarpMode::Mirror, 0.7)
        .lfo(0, LfoWave::Triangle, 0.07)
        .route(ModSource::Lfo(0), ModDest::OscWarp(A as u8), 0.6)
        .fx(reverb(1.0, 0.6))
        .out(12.5),
    MotionAndMorph: "Counter Field" => atmos(0.4, SvfMode::Notch, 900.0)
        .osc(A, WavetableId::Drawbar, -14.0)
        .warp(A, WarpMode::Mirror, 0.5)
        .route(ModSource::NoteOnCounter, ModDest::OscPosition(A as u8), 0.8)
        .route(ModSource::NoteOnCounter, ModDest::FilterCutoff(0), 0.5)
        .route(ModSource::Random, ModDest::LayerPan(A as u8), 0.8)
        .fx(reverb(1.0, 0.58))
        .out(0.9),

    // ------------------------------------------------------------ Pluck ---
    SyncAndFm: "Sync Stab" => pluck(WavetableId::Saw, 5_000.0, 0.22)
        .warp(A, WarpMode::Sync, 0.35)
        .route(ModSource::Envelope(1), ModDest::OscWarp(A as u8), 0.7)
        .route(ModSource::Velocity, ModDest::OscWarp(A as u8), 0.3)
        .fx(delay(NoteDivision::EighthDotted, 0.3, 0.2))
        .out(4.4),
    SyncAndFm: "FM Pluck" => pluck(WavetableId::Sine, 8_000.0, 0.3)
        .warp(A, WarpMode::Fm, 0.5)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 14)
        .route(ModSource::Envelope(1), ModDest::OscWarp(A as u8), 0.75)
        .out(2.6),
    SyncAndFm: "Crush Pluck" => pluck(WavetableId::Bitwave, 6_000.0, 0.25)
        .warp(A, WarpMode::Quantise, 0.5)
        .route(ModSource::Velocity, ModDest::OscWarp(A as u8), -0.3)
        .fx(crush(10.0, 18_000.0, 0.25))
        .out(12.6),
    Expressive: "Drift Pluck" => pluck(WavetableId::Glass, 6_500.0, 0.35)
        .route(ModSource::Random, ModDest::LayerPitch(A as u8), 0.01)
        .route(ModSource::Random, ModDest::LayerPan(A as u8), 0.8)
        .route(ModSource::Random, ModDest::OscPosition(A as u8), 0.3)
        .fx(ping_pong(NoteDivision::Sixteenth, 0.3, 0.25))
        .out(10.1),
    Expressive: "Touch Pluck" => pluck(WavetableId::SubTri, 700.0, 2.4)
        .route(ModSource::NoteModX, ModDest::OscPosition(A as u8), 0.8)
        .route(ModSource::NoteModY, ModDest::FilterCutoff(0), 0.7)
        .out(2.3),
    Expressive: "Wide Stab" => pluck(WavetableId::Wide, 5_200.0, 0.25)
        .uni(A, 3, 13.0)
        .width(A, 1.0)
        .blend(A, 0.85)
        .route(ModSource::Envelope(1), ModDest::OscUnisonBlend(A as u8), 0.5)
        .fx(reverb(0.6, 0.25))
        .out(21.0),

    // ------------------------------------------------------------- Keys ---
    SyncAndFm: "FM Keys" => electric_piano(0.5, 1.6)
        .warp(A, WarpMode::Fm, 0.3)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 19)
        .route(ModSource::Velocity, ModDest::OscWarp(A as u8), 0.5)
        .out(-1.5),
    MotionAndMorph: "Morph Keys" => init()
        .osc(A, WavetableId::AnalogMorph, -12.0)
        .off(B).off(C).off(SUB)
        .route(ModSource::Velocity, ModDest::OscPosition(A as u8), 0.6)
        .route(ModSource::NoteModX, ModDest::OscPosition(A as u8), 0.5)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 3_200.0, 0.4)
        .amp(0.003, 1.4, 0.0, 0.35)
        .fx(reverb(0.6, 0.25))
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.5)
        .route(ModSource::Macro(0), ModDest::OscPosition(A as u8), 0.7)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.5)
        .mac(0, "Morph").mac(1, "Tone")
        .out(9.9),
    Expressive: "Drift Rhodes" => electric_piano(0.35, 2.2)
        .route(ModSource::Random, ModDest::LayerPitch(A as u8), 0.006)
        .route(ModSource::Random, ModDest::LayerPan(A as u8), 0.5)
        .fx(chorus(3, 0.3))
        .out(-1.5),

    // ------------------------------------------------------ Chip & Retro ---
    BassMusic: "Sub Square Bass" => chip(WavetableId::SubSquare, 0.08)
        .semis(A, -12)
        .stepped(ModSource::Envelope(1), ModDest::LayerPitch(A as u8), 0.1, 3)
        .out(-5.6),
    SyncAndFm: "Quantise Blip" => chip(WavetableId::NesPulse50, 0.06)
        .warp(A, WarpMode::Quantise, 0.6)
        .route(ModSource::Lfo(0), ModDest::OscWarp(A as u8), 0.4)
        .out(-5.4),
    MotionAndMorph: "Counter Arp" => chip(WavetableId::C64, 0.07)
        .stepped(ModSource::NoteOnCounter, ModDest::LayerPitch(A as u8), 0.14, 6)
        .route(ModSource::NoteOnCounter, ModDest::OscPosition(A as u8), 0.5)
        .out(2.5),

    // ------------------------------------------------------------ World ---
    // Instruments the orchestra shelf does not have. Synthesised rather than
    // sampled, so none of them is a forgery — what each is after is the
    // *gesture*: which end of the note the energy is at, whether it buzzes,
    // and what it does while it is held.
    World: "Duduk" => lead(WavetableId::Vowel, 1_900.0, 0.04)
        .pos(A, 0.3)
        .noise(0.4, -38.0)
        .filter_route(NOISE, FilterRoute::F1)
        .amp(0.09, 0.0, 1.0, 0.35)
        .lfo(0, LfoWave::Sine, 5.2)
        .late(0, 0.35, 0.5)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.004)
        .mono(0.05)
        .fx(reverb(0.7, 0.3))
        .out(19.0),
    World: "Erhu" => lead(WavetableId::Saw, 3_400.0, 0.05)
        .amp(0.06, 0.0, 1.0, 0.25)
        .lfo(0, LfoWave::Sine, 6.4)
        .late(0, 0.2, 0.3)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.007)
        .route(ModSource::Aftertouch, ModDest::LfoDepth(0), 0.7)
        .mono(0.07)
        .fx(reverb(0.6, 0.25))
        .out(6.0),
    World: "Shamisen" => pluck(WavetableId::Grit, 5_200.0, 0.22)
        .pos(A, 0.6)
        .warp(A, WarpMode::Bend, 0.35)
        .route(ModSource::Velocity, ModDest::OscWarp(A as u8), 0.4)
        .out(0.8),
    World: "Guzheng" => pluck(WavetableId::Struck, 7_500.0, 0.85)
        .pos(A, 0.25)
        .uni(A, 2, 3.0)
        .fx(reverb(0.6, 0.25))
        .out(13.1),
    World: "Oud" => pluck(WavetableId::Odd, 2_600.0, 0.5)
        .pos(A, 0.45)
        .uni(A, 2, 5.0)
        .fx(reverb(0.5, 0.2))
        .out(1.9),
    World: "Kora" => pluck(WavetableId::Glass, 9_500.0, 1.5)
        .fine(A, 3.0)
        .fx(ping_pong(NoteDivision::Eighth, 0.25, 0.2))
        .fx(reverb(0.7, 0.3))
        .out(7.3),
    World: "Balafon" => bell(WavetableId::Tine, 0.26)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 7_500.0, 0.3)
        .filter_route(A, FilterRoute::F1)
        .fx(reverb(0.5, 0.22))
        .out(10.9),
    World: "Gamelan" => bell(WavetableId::Gong, 2.8)
        .warp(A, WarpMode::Rm, 0.3)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 8)
        .fx(reverb(0.85, 0.4))
        .out(15.8),
    World: "Steel Pan" => bell(WavetableId::FmBell, 1.0)
        .pos(A, 0.4)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 1_800.0, 0.5)
        .filter_route(A, FilterRoute::F1)
        .fx(reverb(0.6, 0.28))
        .out(25.6),
    World: "Hurdy Gurdy" => pad(WavetableId::Sawstack, 2_100.0, 0.02, 0.3)
        .uni(A, 3, 11.0)
        .osc(B, WavetableId::Odd, -18.0)
        .semis(B, 7)
        .lfo(0, LfoWave::Triangle, 7.5)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.2)
        .fx(reverb(0.6, 0.25))
        .out(0.1),
    World: "Didgeridoo" => bass(WavetableId::Growl, 620.0, 0.4)
        .pos(A, 0.55)
        .lfo(0, LfoWave::Triangle, 3.1)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.6)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.35)
        .amp(0.05, 0.0, 1.0, 0.4)
        .mono(0.06)
        .out(8.7),
    World: "Bagpipe" => pad(WavetableId::Odd, 4_200.0, 0.01, 0.15)
        .uni(A, 2, 7.0)
        .osc(B, WavetableId::Square, -20.0)
        .semis(B, -12)
        .lfo(0, LfoWave::Sine, 4.5)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.003)
        .out(-1.0),
    World: "Ney" => lead(WavetableId::Hollow, 5_500.0, 0.02)
        .noise(0.35, -34.0)
        .filter_route(NOISE, FilterRoute::F1)
        .amp(0.05, 0.0, 1.0, 0.2)
        .lfo(0, LfoWave::Sine, 5.8)
        .late(0, 0.3, 0.4)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.005)
        .fx(reverb(0.75, 0.32))
        .out(3.9),
    World: "Tabla Tone" => pluck(WavetableId::SubTri, 1_300.0, 0.3)
        .env(2, 0.0, 0.07, 0.0, 0.05)
        // The bend on the head is the whole sound: a tabla is a drum that
        // plays a *note* and then leaves it.
        .route(ModSource::Envelope(2), ModDest::LayerPitch(A as u8), 0.055)
        .out(4.2),

    // -------------------------------------------------------- Cinematic ---
    // A cue is not a chord. These are the shapes a picture asks for: things
    // that arrive, things that hang, and things that hit — spread hard across
    // decay and brightness so a scene can be built out of one shelf.
    Cinematic: "Braam" => pad(WavetableId::Sawstack, 750.0, 0.03, 1.4)
        .uni(A, 5, 16.0)
        .osc(B, WavetableId::SubSaw, -11.0)
        .semis(B, -12)
        .env(2, 0.0, 0.5, 0.0, 0.3)
        .route(ModSource::Envelope(2), ModDest::LayerPitch(A as u8), 0.02)
        .route(ModSource::Envelope(2), ModDest::FilterCutoff(0), 0.5)
        .amp(0.01, 0.0, 1.0, 1.2)
        .fx(drive_fx(DistortionCurve::Tube, 9.0, 0.35))
        .fx(reverb(0.9, 0.4))
        .out(-3.0),
    Cinematic: "Sub Boom" => bass(WavetableId::SubSine, 260.0, 2.2)
        .env(2, 0.0, 0.7, 0.0, 0.4)
        .route(ModSource::Envelope(2), ModDest::LayerPitch(A as u8), 0.11)
        .amp(0.002, 2.4, 0.0, 1.6)
        .fx(reverb(0.8, 0.2))
        .out(0.9),
    Cinematic: "Tension Bed" => pad(WavetableId::Hollow, 1_500.0, 2.5, 4.0)
        .uni(A, 3, 9.0)
        .osc(B, WavetableId::Odd, -22.0)
        .fine(B, 14.0)
        .lfo(0, LfoWave::Triangle, 0.05)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.4)
        .route(ModSource::Random, ModDest::LayerPitch(B as u8), 0.01)
        .fx(reverb(1.0, 0.55))
        .out(25.0),
    Cinematic: "Trailer Hit" => pluck(WavetableId::Grit, 3_200.0, 0.16)
        .osc(B, WavetableId::SubSine, -8.0)
        .semis(B, -24)
        .env(2, 0.0, 0.25, 0.0, 0.1)
        .route(ModSource::Envelope(2), ModDest::LayerPitch(B as u8), 0.06)
        .fx(drive_fx(DistortionCurve::Diode, 11.0, 0.3))
        .fx(reverb(0.95, 0.45))
        .out(-3.0),
    Cinematic: "Rise Swell" => pad(WavetableId::BrightStack, 5_000.0, 3.5, 1.0)
        .uni(A, 4, 13.0)
        .env(2, 4.5, 0.0, 1.0, 0.5)
        .route(ModSource::Envelope(2), ModDest::LayerPitch(A as u8), 0.05)
        .route(ModSource::Envelope(2), ModDest::FilterCutoff(0), 0.8)
        .route(ModSource::Envelope(2), ModDest::OscUnisonDetune(A as u8), 0.7)
        .fx(reverb(1.0, 0.5))
        .out(22.6),
    Cinematic: "Doom Bell" => bell(WavetableId::Gong, 4.5)
        .semis(A, -12)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 1_100.0, 0.3)
        .filter_route(A, FilterRoute::F1)
        .fx(reverb(1.0, 0.55))
        .out(23.8),
    Cinematic: "Hybrid Stab" => pluck(WavetableId::BrightStack, 8_500.0, 0.13)
        .uni(A, 3, 12.0)
        .osc(B, WavetableId::Saw, -18.0)
        .semis(B, 12)
        .fx(reverb(0.7, 0.3))
        .out(8.3),
    Cinematic: "Pulse Bed" => pad(WavetableId::Pulse, 2_400.0, 0.3, 0.8)
        .lfo_sync(0, LfoWave::Square, NoteDivision::Eighth)
        .lfo_mode(0, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.85)
        .smooth(0, 0.09)
        .fx(ping_pong(NoteDivision::Eighth, 0.35, 0.3))
        .fx(reverb(0.85, 0.35))
        .out(-11.1),
    Cinematic: "Signal" => bell(WavetableId::Sine, 1.4)
        .semis(A, 24)
        .lfo_sync(0, LfoWave::Square, NoteDivision::Half)
        .lfo_mode(0, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.9)
        .fx(ping_pong(NoteDivision::QuarterDotted, 0.5, 0.4))
        .fx(reverb(1.0, 0.5))
        .out(0.8),
    Cinematic: "Metal Impact" => pluck(WavetableId::Crunch, 6_000.0, 0.4)
        .warp(A, WarpMode::Rm, 0.6)
        .modulator(A, B)
        .osc(B, WavetableId::Gong, SILENT_DB)
        .semis(B, 5)
        .fx(reverb(0.95, 0.5))
        .out(11.9),
    Cinematic: "String Ostinato" => strings(4_000.0, 0.01, 0.12)
        .lfo_sync(0, LfoWave::Square, NoteDivision::Sixteenth)
        .lfo_mode(0, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.8)
        .smooth(0, 0.05)
        .fx(reverb(0.8, 0.32))
        .out(-13.2),
    Cinematic: "Whale" => atmos(0.6, SvfMode::Lowpass, 900.0)
        .osc(A, WavetableId::Sine, -8.0)
        .lfo(0, LfoWave::Triangle, 0.11)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.04)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.5)
        .fx(reverb(1.0, 0.6))
        .out(19.2),
    Cinematic: "Dark Choir" => choir(0.15, 1.6)
        .semis(A, -12)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 1_300.0, 0.25)
        .fx(reverb(1.0, 0.55))
        .out(32.0),
    Cinematic: "Air Tension" => atmos(0.15, SvfMode::Highpass, 3_000.0)
        .lfo(0, LfoWave::Triangle, 0.08)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.6)
        .route(ModSource::Random, ModDest::LayerPan(NOISE as u8), 0.7)
        .fx(reverb(1.0, 0.5))
        .out(-3.3),

    // ----------------------------------------------------- Lo-Fi & Tape ---
    // Everything here is a *defect* on purpose: bit depth, tape speed, a
    // converter that could not keep up, and a top end that never made it.
    LoFiAndTape: "Tape Keys" => electric_piano(0.65, 0.85)
        .lfo(0, LfoWave::Triangle, 0.7)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.004)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 6_500.0, 0.2)
        .filter_route(A, FilterRoute::F1)
        .fx(crush(11.0, 22_050.0, 0.3))
        .fx(chorus(2, 0.25))
        .out(10.5),
    LoFiAndTape: "Dusty Rhodes" => electric_piano(0.1, 3.8)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 780.0, 0.2)
        .filter_route(A, FilterRoute::F1)
        .noise(0.8, -44.0)
        .filter_route(NOISE, FilterRoute::F1)
        .lfo(0, LfoWave::Triangle, 0.28)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.25)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.006)
        .fx(drive_fx(DistortionCurve::Tube, 7.0, 0.3))
        .out(20.5),
    LoFiAndTape: "Cassette Pad" => pad(WavetableId::AnalogMorph, 3_600.0, 0.35, 1.1)
        .uni(A, 3, 8.0)
        .lfo(0, LfoWave::Triangle, 1.6)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.014)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.4)
        .route(ModSource::Random, ModDest::LayerPitch(A as u8), 0.005)
        .fx(crush(10.0, 16_000.0, 0.25))
        .fx(reverb(0.8, 0.35))
        .out(7.1),
    LoFiAndTape: "Vinyl Bells" => bell(WavetableId::Glass, 1.2)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 3_400.0, 0.2)
        .filter_route(A, FilterRoute::F1)
        .noise(0.9, -40.0)
        .filter_route(NOISE, FilterRoute::F1)
        .fx(crush(9.0, 12_000.0, 0.3))
        .out(23.5),
    LoFiAndTape: "Warble Bass" => bass(WavetableId::SubTri, 900.0, 0.35)
        .lfo(0, LfoWave::Triangle, 0.9)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.008)
        .fx(crush(10.0, 14_000.0, 0.25))
        .out(-0.1),
    LoFiAndTape: "VHS Lead" => lead(WavetableId::Pulse, 2_800.0, 0.03)
        .pos(A, 0.3)
        .lfo(0, LfoWave::Triangle, 1.4)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.007)
        .fx(crush(8.0, 11_025.0, 0.4))
        .fx(chorus(2, 0.3))
        .out(14.3),
    LoFiAndTape: "Wow Pad" => pad(WavetableId::Choir, 1_200.0, 2.2, 3.4)
        .uni(A, 4, 10.0)
        .lfo(0, LfoWave::Sine, 0.35)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.01)
        .fx(reverb(0.9, 0.42))
        .out(21.5),
    LoFiAndTape: "Crackle Bed" => atmos(0.85, SvfMode::Lowpass, 3_000.0)
        .lfo(0, LfoWave::SampleHold, 26.0)
        // The crackle is the *filter* jumping, not the level: a bipolar LFO on
        // a layer's gain walks it under the floor and the layer is skipped.
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.6)
        .fx(crush(8.0, 12_000.0, 0.35))
        .out(-2.5),
    LoFiAndTape: "Old Radio" => lead(WavetableId::Saw, 1_600.0, 0.02)
        .filter(0, FilterModel::Clean, SvfMode::Bandpass, 1_400.0, 0.75)
        .noise(0.5, -40.0)
        .filter_route(NOISE, FilterRoute::F1)
        .fx(crush(7.0, 9_000.0, 0.45))
        .out(21.7),
    LoFiAndTape: "Bit Piano" => pluck(WavetableId::Bitwave, 2_600.0, 1.3)
        .warp(A, WarpMode::Quantise, 0.5)
        .pos(A, 0.35)
        .fx(crush(4.0, 6_000.0, 0.75))
        .out(12.7),
    LoFiAndTape: "Slow Tape" => pad(WavetableId::SubSaw, 560.0, 4.0, 5.5)
        .uni(A, 3, 9.0)
        .lfo(0, LfoWave::Triangle, 0.18)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.012)
        .fx(reverb(1.0, 0.5))
        .out(19.9),
    LoFiAndTape: "Broken Chorus" => pluck(WavetableId::Tine, 4_400.0, 0.7)
        .uni(A, 3, 26.0)
        .route(ModSource::Random, ModDest::OscUnisonDetune(A as u8), 0.6)
        .fx(chorus(3, 0.45))
        .fx(crush(11.0, 20_000.0, 0.2))
        .out(5.6),
    LoFiAndTape: "Muffled Keys" => electric_piano(0.15, 3.4)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 700.0, 0.15)
        .filter_route(A, FilterRoute::F1)
        .fx(reverb(0.7, 0.3))
        .out(10.0),

    // ---------------------------------------------------------- Modular ---
    // Patches that **play themselves**. Hold one note and something happens;
    // hold a chord and four somethings happen, because every source here is
    // per voice. This is the shelf `Random` and `NoteOnCounter` were waiting
    // for — a synth whose every note is identical is a sampler with extra
    // steps.
    Modular: "Random Voltage" => init()
        .osc(A, WavetableId::Square, -13.0)
        .off(B).off(C).off(SUB)
        .lfo_sync(0, LfoWave::SampleHold, NoteDivision::Sixteenth)
        .lfo_mode(0, LfoMode::Free)
        .stepped(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.14, 7)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.4)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 2_600.0, 0.5)
        .amp(0.002, 0.0, 1.0, 0.12)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.35)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.6)
        .route(ModSource::Macro(1), ModDest::FilterResonance(0), 0.5)
        .mac(0, "Clock").mac(1, "Bite")
        .fx(ping_pong(NoteDivision::Sixteenth, 0.35, 0.28))
        .out(3.3),
    Modular: "Krell" => init()
        .osc(A, WavetableId::Triangle, -12.0)
        .off(B).off(C).off(SUB)
        // The Krell patch: every note decides its own length and colour when
        // it starts. `Random` is per voice, so a chord is four decisions.
        .route(ModSource::Random, ModDest::EnvelopeStageTime(0, 3), 0.8)
        .route(ModSource::Random, ModDest::FilterCutoff(0), 0.7)
        .route(ModSource::Random, ModDest::LayerPan(A as u8), 0.8)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 1_800.0, 0.45)
        .amp(0.01, 1.6, 0.0, 1.2)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.5)
        .route(ModSource::Macro(1), ModDest::EnvelopeStageTime(0, 3), 0.6)
        .mac(0, "Colour").mac(1, "Length")
        .fx(reverb(0.95, 0.45))
        .out(8.4),
    Modular: "Turing Pluck" => pluck(WavetableId::Pulse, 4_800.0, 0.3)
        // A shift register: the sequence advances one place per note played
        // and comes back round, which is what a Turing machine module is.
        .stepped(ModSource::NoteOnCounter, ModDest::LayerPitch(A as u8), 0.12, 8)
        .route(ModSource::NoteOnCounter, ModDest::OscPosition(A as u8), 0.6)
        .fx(ping_pong(NoteDivision::Eighth, 0.35, 0.3))
        .out(14.3),
    Modular: "Clock Divide" => init()
        .osc(A, WavetableId::Saw, -13.0)
        .off(B).off(C).off(SUB)
        .lfo_sync(0, LfoWave::Square, NoteDivision::Eighth)
        .lfo_sync(1, LfoWave::Square, NoteDivision::Half)
        .lfo_mode(0, LfoMode::Free)
        .lfo_mode(1, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.7)
        // The slow gate opens the fast one — two divisions of one clock, which
        // is the whole of a divider.
        .route_via(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.7, ModSource::Lfo(1))
        .smooth(0, 0.05)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 2_200.0, 0.5)
        .amp(0.004, 0.0, 1.0, 0.2)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.35)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.6)
        .route(ModSource::Macro(1), ModDest::LfoRate(1), 0.6)
        .mac(0, "Fast").mac(1, "Slow")
        .out(-9.2),
    Modular: "Slew Bass" => bass(WavetableId::SubSaw, 1_200.0, 0.3)
        .lfo(0, LfoWave::SampleHold, 3.4)
        .smooth(0, 0.85)
        // Sample and hold through a slew limiter is a *glide* between random
        // values rather than a jump — the sound of a portamento CV.
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.7)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.01)
        .out(-0.8),
    Modular: "Chaos Lead" => lead(WavetableId::Grit, 5_500.0, 0.0)
        .route(ModSource::Random, ModDest::OscPosition(A as u8), 0.8)
        .route(ModSource::Random, ModDest::OscWarp(A as u8), 0.5)
        .route(ModSource::Random, ModDest::LayerPan(A as u8), 0.9)
        .warp(A, WarpMode::Bend, 0.3)
        .amp(0.003, 0.0, 1.0, 0.15)
        .fx(delay(NoteDivision::SixteenthTriplet, 0.4, 0.25))
        .out(8.2),
    Modular: "Ping Filter" => init()
        .off(A).off(B).off(C).off(SUB)
        .noise(0.2, -14.0)
        .filter_route(NOISE, FilterRoute::F1)
        // A resonant filter struck by a burst of noise rings at its corner —
        // a whole voice made of one filter, which is how a ping module works.
        .filter(0, FilterModel::Ladder, SvfMode::Bandpass, 900.0, 0.95)
        .key_track(0, 1.0)
        .amp(0.0005, 0.05, 0.0, 0.04)
        .env(1, 0.0, 0.04, 0.0, 0.03)
        .env_to_cut(0.3)
        .route(ModSource::Velocity, ModDest::FilterResonance(0), 0.2)
        .route(ModSource::Macro(0), ModDest::FilterResonance(0), 0.4)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.5)
        .mac(0, "Ring").mac(1, "Pitch")
        .fx(reverb(0.8, 0.35))
        .out(-7.7),
    Modular: "Burst" => init()
        .osc(A, WavetableId::Sine, -12.0)
        .off(B).off(C).off(SUB)
        .lfo(0, LfoWave::Square, 22.0)
        .env(2, 0.0, 0.5, 0.0, 0.2)
        // A burst generator: a fast gate that only exists while the envelope
        // that opened it is still up.
        .route_via(ModSource::Lfo(0), ModDest::Amp, 0.95, ModSource::Envelope(2))
        .route(ModSource::Envelope(2), ModDest::LfoRate(0), -0.5)
        .filter(0, FilterModel::Clean, SvfMode::Lowpass, 6_000.0, 0.3)
        .amp(0.001, 0.0, 1.0, 0.1)
        .route(ModSource::Velocity, ModDest::LfoRate(0), 0.4)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.7)
        .route(ModSource::Macro(1), ModDest::EnvelopeStageTime(2, 2), 0.6)
        .mac(0, "Rate").mac(1, "Length")
        .fx(ping_pong(NoteDivision::ThirtySecond, 0.3, 0.25))
        .out(-15.6),
    Modular: "Wander Pad" => pad(WavetableId::AnalogMorph, 2_400.0, 1.8, 3.6)
        .uni(A, 3, 7.0)
        .lfo(0, LfoWave::SampleHold, 0.22)
        .smooth(0, 0.95)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.8)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.5)
        .route(ModSource::Random, ModDest::LayerPan(A as u8), 0.7)
        .fx(reverb(1.0, 0.5))
        .out(13.6),
    Modular: "Divider Bass" => bass(WavetableId::Square, 1_500.0, 0.2)
        .lfo_sync(0, LfoWave::Square, NoteDivision::Quarter)
        .lfo_mode(0, LfoMode::Free)
        .stepped(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.1, 2)
        .osc(B, WavetableId::SubSquare, -14.0)
        .semis(B, -12)
        .out(-4.5),
    Modular: "Sync Ramp" => lead(WavetableId::SyncSweep, 6_500.0, 0.0)
        .warp(A, WarpMode::Sync, 0.15)
        .lfo(0, LfoWave::SawUp, 0.6)
        .route(ModSource::Lfo(0), ModDest::OscWarp(A as u8), 0.85)
        .amp(0.01, 0.0, 1.0, 0.3)
        .fx(delay(NoteDivision::Quarter, 0.35, 0.25))
        .out(3.8),
    Modular: "Noise Voltage" => init()
        .off(A).off(B).off(C).off(SUB)
        .noise(0.45, -13.0)
        .filter_route(NOISE, FilterRoute::F1)
        .filter(0, FilterModel::Ladder, SvfMode::Bandpass, 1_600.0, 0.8)
        .lfo(0, LfoWave::SampleHold, 7.0)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.9)
        .amp(0.02, 0.0, 1.0, 0.4)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.3)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.7)
        .route(ModSource::Macro(1), ModDest::FilterResonance(0), 0.5)
        .mac(0, "Rate").mac(1, "Ring")
        .fx(reverb(0.85, 0.4))
        .out(4.6),
    Modular: "Patch Bay" => init()
        .osc(A, WavetableId::AnalogMorph, -13.0)
        .osc(B, WavetableId::Odd, -20.0)
        .semis(B, 7)
        .off(C).off(SUB)
        .lfo_sync(0, LfoWave::Triangle, NoteDivision::HalfTriplet)
        .lfo(1, LfoWave::SampleHold, 5.0)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.7)
        .route(ModSource::Lfo(1), ModDest::OscPosition(B as u8), 0.5)
        .route(ModSource::Lfo(0), ModDest::LfoRate(1), 0.6)
        .route(ModSource::Random, ModDest::OscPosition(B as u8), 0.4)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 3_000.0, 0.45)
        .character(0, 0.4)
        .amp(0.05, 0.0, 1.0, 0.7)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.35)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.6)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.6)
        .mac(0, "Clock").mac(1, "Tone")
        .fx(reverb(0.9, 0.42))
        .out(5.2),

    // ---- Modular, second pass: the rest of the rack ---------------------
    //
    // Thirteen was the idea; this is the variety. Each of these is a *module*
    // somebody would recognise rather than another setting — a low-pass gate,
    // a wavefolder, a rungler, a complex oscillator, an undertone divider — and
    // they are spread on purpose across how long they ring and how bright they
    // are, because `every_pair_in_a_category_is_audibly_apart` reads those two
    // before it reads anything else and a shelf this size runs out of room
    // fast.
    Modular: "Low-Pass Gate" => pluck(WavetableId::SubTri, 2_400.0, 0.32)
        // A vactrol opens the filter and the amp *together*, which is why a
        // Buchla pluck gets quieter and darker at the same rate. One envelope
        // on both is the whole module.
        .env(1, 0.0, 0.3, 0.0, 0.12)
        .env_to_cut(0.95)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.5)
        .out(4.0),
    Modular: "Wavefolder" => lead(WavetableId::Triangle, 16_000.0, 0.0)
        // Folding a triangle is the West Coast way to get harmonics: no filter
        // takes anything away, the wave just gains corners.
        .warp(A, WarpMode::Mirror, 0.2)
        .env(2, 0.8, 0.0, 1.0, 0.4)
        .route(ModSource::Envelope(2), ModDest::OscWarp(A as u8), 0.8)
        .route(ModSource::ModWheel, ModDest::OscWarp(A as u8), 0.6)
        .amp(0.004, 0.6, 0.35, 0.25)
        .out(5.0),
    Modular: "Rungler" => init()
        .osc(A, WavetableId::Square, -13.0)
        .osc(B, WavetableId::Square, -19.0)
        .semis(B, 5)
        .off(C).off(SUB)
        // Benjolin's trick: two oscillators that modulate each other through a
        // shift register, so it is never random and never repeats either.
        .lfo(0, LfoWave::Square, 6.3)
        .lfo(1, LfoWave::SampleHold, 11.7)
        .route(ModSource::Lfo(0), ModDest::LfoRate(1), 0.7)
        .route(ModSource::Lfo(1), ModDest::LfoRate(0), 0.6)
        .stepped(ModSource::Lfo(1), ModDest::LayerPitch(A as u8), 0.16, 6)
        .route(ModSource::Lfo(0), ModDest::OscPosition(B as u8), 0.5)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 3_400.0, 0.55)
        .amp(0.003, 0.0, 1.0, 0.14)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.4)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.7)
        .route(ModSource::Macro(1), ModDest::LfoRate(1), 0.7)
        .mac(0, "Chaos").mac(1, "Rate")
        .fx(delay(NoteDivision::SixteenthTriplet, 0.35, 0.22))
        .out(3.2),
    Modular: "Complex Osc" => lead(WavetableId::Sine, 7_000.0, 0.0)
        // Buchla's 259: one oscillator's only job is to bend the other's
        // phase, and the *index* is the timbre knob.
        .warp(A, WarpMode::Fm, 0.35)
        .modulator(A, B)
        .osc(B, WavetableId::Sine, SILENT_DB)
        .semis(B, 7)
        .lfo(0, LfoWave::Triangle, 0.25)
        .route(ModSource::Lfo(0), ModDest::OscWarp(A as u8), 0.55)
        .route(ModSource::Lfo(0), ModDest::LayerPitch(B as u8), 0.03)
        .amp(0.05, 0.0, 1.0, 0.9)
        .fx(reverb(0.7, 0.3))
        .out(1.4),
    Modular: "Bouncing Ball" => init()
        .osc(A, WavetableId::Triangle, -12.0)
        .off(B).off(C).off(SUB)
        .lfo(0, LfoWave::Square, 3.0)
        .env(2, 0.0, 1.8, 0.0, 0.4)
        // The gate speeds up as the envelope falls, which is a ball losing
        // height — one envelope doing two jobs at once.
        .inverted(ModSource::Envelope(2), ModDest::LfoRate(0), 0.9)
        .route_via(ModSource::Lfo(0), ModDest::Amp, 0.95, ModSource::Envelope(2))
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 4_500.0, 0.35)
        .amp(0.002, 2.0, 0.0, 0.3)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.4)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.6)
        .route(ModSource::Macro(1), ModDest::EnvelopeStageTime(2, 2), 0.7)
        .mac(0, "Bounce").mac(1, "Fall")
        .out(-12.2),
    Modular: "Undertone" => bass(WavetableId::SubSquare, 520.0, 0.9)
        // A sub-harmonicon divides *down* rather than multiplying up, so its
        // intervals are the undertone series and none of them is tempered.
        .osc(B, WavetableId::SubTri, -14.0)
        .semis(B, -12)
        .osc(C, WavetableId::SubSine, -17.0)
        .semis(C, -19)
        .lfo_sync(0, LfoWave::Square, NoteDivision::Quarter)
        .lfo_mode(0, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::LayerGain(C as u8), 0.35)
        .amp(0.02, 0.0, 1.0, 0.7)
        .out(-28.1),
    Modular: "Feedback Patch" => init()
        .off(A).off(B).off(C).off(SUB)
        .noise(0.3, -30.0)
        .filter_route(NOISE, FilterRoute::F1)
        // A filter turned up until it sings: the noise is only there to start
        // it, and what you hear is the resonance.
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 700.0, 0.98)
        .key_track(0, 1.0)
        .lfo(0, LfoWave::Triangle, 0.09)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.45)
        .amp(0.4, 0.0, 1.0, 2.2)
        .route(ModSource::Velocity, ModDest::FilterResonance(0), 0.15)
        .route(ModSource::Macro(0), ModDest::FilterCutoff(0), 0.6)
        .route(ModSource::Macro(1), ModDest::FilterResonance(0), 0.25)
        .mac(0, "Pitch").mac(1, "Sing")
        .fx(reverb(0.9, 0.45))
        .out(-22.7),
    Modular: "Vactrol Bongo" => pluck(WavetableId::Sine, 5_400.0, 0.17)
        .env(2, 0.0, 0.04, 0.0, 0.03)
        // A drum, not a gate: nearly all of this is the head falling in pitch
        // over forty milliseconds.
        .route(ModSource::Envelope(2), ModDest::LayerPitch(A as u8), 0.11)
        .route(ModSource::Random, ModDest::LayerPitch(A as u8), 0.02)
        .out(4.0),
    Modular: "Serge Resonant" => pad(WavetableId::Odd, 1_900.0, 0.15, 0.8)
        .osc(B, WavetableId::Vowel, -17.0)
        .pos(B, 0.55)
        .filter(0, FilterModel::Formant, SvfMode::Bandpass, 1_100.0, 0.6)
        .character(0, 0.55)
        .lfo(0, LfoWave::Triangle, 0.17)
        .route(ModSource::Lfo(0), ModDest::FilterCharacter(0), 0.6)
        .out(5.1),
    Modular: "Quantised Melody" => pluck(WavetableId::Saw, 4_000.0, 0.26)
        .lfo_sync(0, LfoWave::SampleHold, NoteDivision::Eighth)
        .lfo_mode(0, LfoMode::Free)
        // Five steps, so what comes out is a pentatonic and every accident is
        // still in key — the reason a quantiser is in every rack.
        .stepped(ModSource::Lfo(0), ModDest::LayerPitch(A as u8), 0.1, 5)
        .fx(ping_pong(NoteDivision::Eighth, 0.35, 0.28))
        .out(6.0),
    Modular: "Ratchet" => init()
        .osc(A, WavetableId::Pulse, -13.0)
        .pos(A, 0.25)
        .off(B).off(C).off(SUB)
        .lfo_sync(0, LfoWave::Square, NoteDivision::ThirtySecond)
        .lfo_sync(1, LfoWave::Square, NoteDivision::Quarter)
        .lfo_mode(0, LfoMode::Free)
        .lfo_mode(1, LfoMode::Free)
        // Bursts inside a step: the fast gate only exists while the slow one
        // is open, which is what a ratchet is.
        .route_via(ModSource::Lfo(0), ModDest::Amp, 0.95, ModSource::Lfo(1))
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 8_000.0, 0.4)
        .amp(0.001, 0.0, 1.0, 0.06)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.4)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.6)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.5)
        .mac(0, "Rate").mac(1, "Tone")
        .fx(ping_pong(NoteDivision::Sixteenth, 0.3, 0.25))
        .out(-7.0),
    Modular: "Clock Swing" => init()
        .osc(A, WavetableId::Sawstack, -13.0)
        .uni(A, 2, 7.0)
        .off(B).off(C).off(SUB)
        .lfo_sync(0, LfoWave::Square, NoteDivision::EighthDotted)
        .lfo_sync(1, LfoWave::Triangle, NoteDivision::Eighth)
        .lfo_mode(0, LfoMode::Free)
        .lfo_mode(1, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::Amp, 0.75)
        .route(ModSource::Lfo(1), ModDest::FilterCutoff(0), 0.55)
        .smooth(0, 0.07)
        .filter(0, FilterModel::Ladder, SvfMode::Lowpass, 2_600.0, 0.45)
        .amp(0.006, 0.0, 1.0, 0.35)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.35)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.6)
        .route(ModSource::Macro(1), ModDest::LfoRate(1), 0.6)
        .mac(0, "Swing").mac(1, "Sweep")
        .out(-5.8),
    Modular: "Drone Cell" => pad(WavetableId::Drawbar, 1_300.0, 3.5, 6.0)
        .uni(A, 3, 5.0)
        .osc(B, WavetableId::Odd, -20.0)
        .fine(B, 9.0)
        .lfo(0, LfoWave::Triangle, 0.03)
        .lfo(1, LfoWave::Sine, 0.047)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.6)
        .route(ModSource::Lfo(1), ModDest::LayerGain(B as u8), 0.3)
        .route(ModSource::Lfo(1), ModDest::FilterCutoff(0), 0.3)
        .fx(reverb(1.0, 0.55))
        .out(18.7),
    Modular: "Attenuverter" => pluck(WavetableId::AnalogMorph, 3_600.0, 0.65)
        .lfo(0, LfoWave::Triangle, 1.1)
        // The same source, one way up and one way down: an attenuverter is
        // how a rack gets two opposite gestures out of one modulator.
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.75)
        .inverted(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.6)
        .out(6.9),
    Modular: "Trigger Echo" => pluck(WavetableId::Glass, 13_000.0, 0.07)
        .route(ModSource::Random, ModDest::LayerPan(A as u8), 0.85)
        .fx(ping_pong(NoteDivision::SixteenthDotted, 0.55, 0.4))
        .out(14.2),
    Modular: "Noise Comparator" => init()
        .off(A).off(B).off(C).off(SUB)
        .noise(0.05, -16.0)
        .filter_route(NOISE, FilterRoute::F1)
        .filter(0, FilterModel::Clean, SvfMode::Highpass, 4_000.0, 0.4)
        .lfo(0, LfoWave::SampleHold, 15.0)
        .route(ModSource::Lfo(0), ModDest::FilterCutoff(0), 0.7)
        .amp(0.002, 0.0, 1.0, 0.1)
        .route(ModSource::Velocity, ModDest::FilterCutoff(0), 0.4)
        .route(ModSource::Macro(0), ModDest::LfoRate(0), 0.7)
        .route(ModSource::Macro(1), ModDest::FilterCutoff(0), 0.5)
        .mac(0, "Rate").mac(1, "Edge")
        .fx(delay(NoteDivision::ThirtySecond, 0.3, 0.2))
        .out(11.0),
    Modular: "Stepped Voltage" => pad(WavetableId::Hollow, 5_600.0, 0.5, 1.4)
        .uni(A, 2, 5.0)
        .lfo_sync(0, LfoWave::SampleHold, NoteDivision::Quarter)
        .lfo_mode(0, LfoMode::Free)
        .route(ModSource::Lfo(0), ModDest::OscPosition(A as u8), 0.7)
        .route(ModSource::Lfo(0), ModDest::LayerPan(A as u8), 0.6)
        .fx(reverb(0.9, 0.45))
        .out(2.9),

}
