//! One addressed parameter, read from or written into a [`Patch`] (TDD §8.2).
//!
//! # Why this is in `fontelle-core` and not in the editor
//!
//! It used to live in `fontelle-app`, beside the panel that draws the knobs,
//! which was the right place while a knob was the only thing that moved one.
//! An **automation lane** moves one too, and it does it on the audio thread —
//! which cannot see `fontelle-app` (INVARIANT 4 runs the dependency the other
//! way). So the mapping lives with the `Patch` it writes to, and both callers
//! use it: the panel to draw and set, [`crate::Sampler::set_patch_param`] to
//! apply what a lane is holding.
//!
//! That is not a tidying-up. It is what makes *"every knob is automatable"*
//! true by construction rather than by two lists agreeing — the panel offers
//! exactly the addresses [`set`] accepts, because it is the same match.
//!
//! # The rules
//!
//! - **Addresses are INVARIANT 7's.** They never change once assigned. One
//!   this build does not recognise changes nothing and is **not an error**
//!   (`set` returns `false`), so a project naming a parameter a later build
//!   dropped still opens.
//! - **Values are normalised**, `0..=1`, in both directions. The mapping onto
//!   hertz or seconds is here; a dial and an automation lane both hand over a
//!   fraction, and the taper is a property of the parameter rather than of
//!   whatever is moving it.
//! - **Read and write go through the same table.** A control whose read-out
//!   disagreed with what it wrote is not expressible: `set` and [`value`] are
//!   the two halves of one match, and `tests/patch_params.rs` checks the round
//!   trip on every one of them.
//! - **It runs on the audio thread**, so it allocates nothing: the address is
//!   split as a `&str` and everything else is a field write (INVARIANT 1).

use fontelle_dsp::{
    FilterModel, FilterRoute, FilterSlope, Interpolation, MAX_UNISON, OscKind, SampleLoop, SvfMode,
    SynthSource, WarpMode, WavetableId,
};
use fontelle_types::{LfoWave, NoteDivision};

use crate::patch::{LfoMode, MACRO_COUNT, Patch, Source};
use crate::playback::PlaybackConfig;

/// The loudest and quietest a level goes, in dB. Shared by a channel's own
/// fader and by a layer's, so the two dials mean the same thing.
pub const GAIN_MIN_DB: f32 = -60.0;
pub const GAIN_MAX_DB: f32 = 12.0;

/// A cutoff runs 20 Hz to 20 kHz, and it runs **logarithmically**: a linear
/// cutoff knob spends nine tenths of its travel above 2 kHz, where almost
/// nothing musically interesting happens.
pub const CUTOFF_MIN_HZ: f32 = 20.0;
pub const CUTOFF_MAX_HZ: f32 = 20_000.0;

/// The longest an envelope stage may be set to. Ten seconds is a pad's
/// release; beyond that a knob is unusable for everything shorter.
pub const ENV_MAX_S: f32 = 10.0;

/// And the longest a glide.
pub const GLIDE_MAX_S: f32 = 2.0;

/// The most voices a patch may be given. §7.4's limit.
pub const MAX_POLYPHONY: f32 = 256.0;

/// How far an oscillator detunes, in cents. A semitone either way: past that it
/// is a transposition, and the octave chooser is right there.
pub const DETUNE_CENTS: f32 = 100.0;

/// The filter modes the panel offers, in the order the chip steps through them.
pub const FILTER_MODES: [(SvfMode, &str); 7] = [
    (SvfMode::Lowpass, "LP"),
    (SvfMode::Highpass, "HP"),
    (SvfMode::Bandpass, "BP"),
    (SvfMode::Notch, "Notch"),
    (SvfMode::Bell, "Bell"),
    (SvfMode::LowShelf, "LoShelf"),
    (SvfMode::HighShelf, "HiShelf"),
];

/// The interpolation choices, with "Session" first: `None` on a layer means it
/// follows the session's own quality (TDD §7.6), which is the default an import
/// produces and a real answer rather than the absence of one.
pub const QUALITIES: [(Option<Interpolation>, &str); 5] = [
    (None, "Session"),
    (Some(Interpolation::Draft), "Draft"),
    (Some(Interpolation::Normal), "Normal"),
    (Some(Interpolation::High), "High"),
    (Some(Interpolation::Ultra), "Ultra"),
];

/// The waveforms an oscillator layer offers, in the order the chooser steps
/// through them. Saw first because it is the one a subtractive patch starts
/// from.
pub const SHAPES: [(OscKind, &str); 5] = [
    (OscKind::Saw, "Saw"),
    (OscKind::Square, "Square"),
    (OscKind::Triangle, "Tri"),
    (OscKind::Sine, "Sine"),
    (OscKind::Noise, "Noise"),
];

/// How far an oscillator may be transposed, in octaves. Two either way is what
/// a sub and a top octave need and further than anybody reaches.
pub const OCTAVES: [i32; 5] = [-2, -1, 0, 1, 2];

/// How far a Flopsynth oscillator transposes, in semitones either way.
///
/// Three octaves rather than the two the `OCTAVES` chooser offers, and in
/// **semitones** rather than octaves, because FM ratios are intervals: the
/// DX e-piano's tine is the modulator 43 semitones above the carrier, and an
/// octave chooser cannot say that.
pub const SEMITONE_RANGE: f32 = 36.0;

/// The widest a pitch bend may be set to, in semitones. Two octaves is what a
/// whammy patch wants and further than a keyboard sends.
pub const BEND_MAX_SEMITONES: f32 = 24.0;

/// A patch's output trim. Asymmetric on purpose: presets are loudness-matched
/// *downwards* far more often than up, and the +12 is there for the quiet ones
/// rather than as an invitation.
pub const OUTPUT_MIN_DB: f32 = -24.0;
pub const OUTPUT_MAX_DB: f32 = 12.0;

/// An LFO's free-running rate. From one cycle in a hundred seconds — a pad
/// that never quite repeats — to well past where it stops being a rhythm and
/// starts being a tone.
pub const LFO_MIN_HZ: f32 = 0.01;
pub const LFO_MAX_HZ: f32 = 40.0;

/// The longest an LFO's delay or fade may be.
pub const LFO_TIME_MAX_S: f32 = 10.0;

/// How far a Flopsynth oscillator's unison detunes, in cents.
pub const UNISON_DETUNE_MAX_CENTS: f32 = 100.0;

/// The three voice modes, in the order the chooser steps through them.
pub const VOICE_MODES: [(crate::voice::RetriggerMode, &str); 3] = [
    (crate::voice::RetriggerMode::Poly, "Poly"),
    (crate::voice::RetriggerMode::Mono, "Mono"),
    (crate::voice::RetriggerMode::Legato, "Legato"),
];

// ------------------------------------------------------------- the write ---

/// Applies one control to `patch`. Returns whether anything changed.
///
/// An address that names nothing is ignored — see the module's own docs.
pub fn set(patch: &mut Patch, address: &str, value: f32) -> bool {
    let value = value.clamp(0.0, 1.0);
    match address {
        "patch/voice/polyphony" => {
            let voices = lerp(value, 1.0, MAX_POLYPHONY)
                .round()
                .clamp(1.0, MAX_POLYPHONY);
            patch.voice_config.polyphony = voices as u16;
            true
        }
        "patch/voice/glide" => {
            patch.voice_config.glide_time_s = lerp(value, 0.0, GLIDE_MAX_S);
            true
        }
        "patch/voice/legato" => {
            patch.voice_config.glide_legato_only = value >= 0.5;
            true
        }
        "patch/voice/mode" => {
            patch.voice_config.retrigger = VOICE_MODES[choice_index(value, VOICE_MODES.len())].0;
            true
        }
        "patch/voice/bend_range" => {
            patch.voice_config.bend_range_semitones = lerp(value, 0.0, BEND_MAX_SEMITONES);
            true
        }
        "patch/output" => {
            patch.output_db = lerp(value, OUTPUT_MIN_DB, OUTPUT_MAX_DB);
            true
        }
        "patch/quality" => {
            let index = choice_index(value, QUALITIES.len());
            let quality = QUALITIES[index].0;
            // Every layer: a patch whose layers disagree about interpolation is
            // not something the panel can draw, and §7.6's per-layer pin is a
            // choice made per *patch* far more often than per layer.
            for layer in &mut patch.layers {
                layer.playback = PlaybackConfig {
                    interpolation: quality,
                    ..layer.playback
                };
            }
            true
        }
        _ => {
            if let Some((index, field)) = indexed(address, "patch/filter[") {
                return set_filter(patch, index, field, value);
            }
            if let Some((index, field)) = indexed(address, "patch/env[") {
                return set_envelope(patch, index, field, value);
            }
            if let Some((index, field)) = indexed(address, "patch/layer[") {
                return set_layer(patch, index, field, value);
            }
            if let Some((index, field)) = indexed(address, "patch/lfo[") {
                return set_lfo(patch, index, field, value);
            }
            if let Some((index, field)) = indexed(address, "patch/mod[") {
                if field != "depth" {
                    return false;
                }
                let Some(route) = patch.mod_matrix.routes.get_mut(index) else {
                    return false;
                };
                // Bipolar: a route that could only add would be half a matrix.
                route.depth = value * 2.0 - 1.0;
                return true;
            }
            if let Some((index, field)) = indexed(address, "patch/fx[") {
                return set_fx(patch, index, field, value);
            }
            // `patch/macro[n]` has no field after it — it *is* the knob — so it
            // is matched on its own rather than through `indexed`.
            if let Some(rest) = address.strip_prefix("patch/macro[")
                && let Some(number) = rest.strip_suffix(']')
                && let Ok(index) = number.parse::<usize>()
                && index < MACRO_COUNT
            {
                patch.macros[index].value = value;
                return true;
            }
            false
        }
    }
}

fn set_lfo(patch: &mut Patch, index: usize, field: &str, value: f32) -> bool {
    let Some(lfo) = patch.lfos.get_mut(index) else {
        return false;
    };
    match field {
        "wave" => lfo.wave = LfoWave::ALL[choice_index(value, LfoWave::ALL.len())],
        "rate" => lfo.rate_hz = lerp_log(value, LFO_MIN_HZ, LFO_MAX_HZ),
        "sync" => lfo.sync = value >= 0.5,
        "division" => {
            lfo.division = NoteDivision::ALL[choice_index(value, NoteDivision::ALL.len())];
        }
        "depth" => lfo.depth = value,
        // The same cubic taper an envelope stage gets, and for the same
        // reason: the first tenth of the dial has to cover a vibrato's
        // quarter-second delay and the last has to reach a pad's ten seconds.
        "delay" => lfo.delay_s = value.clamp(0.0, 1.0).powi(3) * LFO_TIME_MAX_S,
        "fade" => lfo.fade_s = value.clamp(0.0, 1.0).powi(3) * LFO_TIME_MAX_S,
        "phase" => lfo.phase = value,
        "mode" => lfo.mode = LfoMode::ALL[choice_index(value, LfoMode::ALL.len())],
        "smooth" => lfo.smooth = value,
        _ => return false,
    }
    true
}

fn set_fx(patch: &mut Patch, index: usize, field: &str, value: f32) -> bool {
    let Some(slot) = patch.fx.get_mut(index) else {
        return false;
    };
    if field == "enabled" {
        slot.enabled = value >= 0.5;
        return true;
    }
    // Everything else is the effect's own parameter, by the id its `ParamSpec`
    // gave it — which is already RT-safe and already the list automation works
    // from, so a patch effect's knobs are automatable the day they exist.
    if slot.config.specs().iter().any(|spec| spec.id == field) {
        slot.config.set_normalised(field, value);
        return true;
    }
    false
}

fn set_filter(patch: &mut Patch, index: usize, field: &str, value: f32) -> bool {
    let Some(filter) = patch.filters.get_mut(index) else {
        return false;
    };
    match field {
        "enabled" => filter.enabled = value >= 0.5,
        "mode" => filter.mode = FILTER_MODES[choice_index(value, FILTER_MODES.len())].0,
        "cutoff" => filter.cutoff_hz = lerp_log(value, CUTOFF_MIN_HZ, CUTOFF_MAX_HZ),
        "resonance" => filter.resonance = value,
        "slope" => filter.slope = FilterSlope::ALL[choice_index(value, FilterSlope::ALL.len())],
        "model" => filter.model = FilterModel::ALL[choice_index(value, FilterModel::ALL.len())],
        "drive" => filter.drive = value,
        "key_track" => filter.key_track = value,
        "character" => filter.character = value,
        _ => return false,
    }
    true
}

fn set_envelope(patch: &mut Patch, index: usize, field: &str, value: f32) -> bool {
    let Some(env) = patch.envelopes.get_mut(index) else {
        return false;
    };
    match field {
        "delay" => env.delay_s = lerp_stage(value),
        "attack" => env.attack_s = lerp_stage(value),
        "hold" => env.hold_s = lerp_stage(value),
        "decay" => env.decay_s = lerp_stage(value),
        "sustain" => env.sustain_level = value,
        "release" => env.release_s = lerp_stage(value),
        // Bipolar: the middle of the dial is the straight line every envelope
        // had before shapes existed.
        "attack_shape" => env.attack_shape = value * 2.0 - 1.0,
        "decay_shape" => env.decay_shape = value * 2.0 - 1.0,
        "release_shape" => env.release_shape = value * 2.0 - 1.0,
        _ => return false,
    }
    true
}

fn set_layer(patch: &mut Patch, index: usize, field: &str, value: f32) -> bool {
    let Some(layer) = patch.layers.get_mut(index) else {
        return false;
    };
    // Flopsynth's own controls live under `synth/`, so that a layer's level
    // and pan stay the addresses they have always been and only what is new
    // is new (INVARIANT 7).
    if let Some(field) = field.strip_prefix("synth/") {
        let Source::Synth(osc) = &mut layer.source else {
            return false;
        };
        return set_synth(osc, field, value);
    }
    let is_oscillator = matches!(layer.source, crate::patch::Source::Oscillator(_));
    let is_synth = matches!(layer.source, Source::Synth(_));
    match field {
        "gain" => layer.gain_db = lerp(value, GAIN_MIN_DB, GAIN_MAX_DB),
        "pan" => layer.pan = value * 2.0 - 1.0,
        // The three below are an oscillator's, and they are refused on a
        // sampled layer rather than silently writing a root key and a tuning
        // that would transpose somebody's piano.
        "shape" => {
            if !is_oscillator {
                return false;
            }
            layer.source =
                crate::patch::Source::Oscillator(SHAPES[choice_index(value, SHAPES.len())].0);
        }
        "octave" => {
            if !is_oscillator {
                return false;
            }
            layer.root_key = root_key_for(OCTAVES[choice_index(value, OCTAVES.len())]);
        }
        "tune" => {
            // Accepted on a synth layer as well as an oscillator one: the fine
            // tune is the *layer's*, not the source's, and a Flopsynth
            // oscillator wants one as much as anything else does. `shape` and
            // `octave` are not extended, because a Flopsynth layer has neither
            // — its waveform is a table and its transposition is
            // `synth/semitones`, and writing a root key nobody can see would
            // be a control that moves the sound and shows nothing.
            if !is_oscillator && !is_synth {
                return false;
            }
            layer.fine_tune_cents = lerp(value, -DETUNE_CENTS, DETUNE_CENTS);
        }
        _ => return false,
    }
    true
}

fn set_synth(osc: &mut fontelle_dsp::SynthOsc, field: &str, value: f32) -> bool {
    match field {
        "table" => {
            // A noise layer has no table, and one given one would stop being
            // the noise layer — which is a change the window has no way to
            // show and nobody asked for.
            if matches!(osc.source, SynthSource::Noise) {
                return false;
            }
            osc.source =
                SynthSource::Table(WavetableId::ALL[choice_index(value, WavetableId::ALL.len())]);
        }
        "position" => osc.position = value,
        "warp_mode" => osc.warp = WarpMode::ALL[choice_index(value, WarpMode::ALL.len())],
        "warp" => osc.warp_amount = value,
        "modulator" => {
            // Position 0 is "none"; the rest are layer indices 1..=4. A
            // modulator that is not a *later* layer is refused by the panel
            // (`flopsynth::apply_edit`), not here — this is the address table,
            // and an address that silently rewrote its own value would be one
            // whose read-out disagreed with what it wrote.
            let index = choice_index(value, MODULATOR_CHOICES);
            osc.modulator = (index > 0).then_some(index as u8);
        }
        "semitones" => {
            osc.semitones = lerp(value, -SEMITONE_RANGE, SEMITONE_RANGE).round() as i8;
        }
        "phase" => osc.phase = value,
        "random_phase" => osc.random_phase = value >= 0.5,
        "key_track" => osc.key_track = value >= 0.5,
        "route" => {
            osc.filter_route = FilterRoute::ALL[choice_index(value, FilterRoute::ALL.len())];
        }
        "unison/voices" => {
            osc.unison.voices = lerp(value, 1.0, MAX_UNISON as f32).round() as u8;
        }
        "unison/detune" => osc.unison.detune_cents = lerp(value, 0.0, UNISON_DETUNE_MAX_CENTS),
        "unison/blend" => osc.unison.blend = value,
        "unison/width" => osc.unison.width = value,
        "noise_colour" => osc.noise_colour = value,
        // Which of the three kinds of source this oscillator is (table,
        // recording, string). Refused on the noise layer for `table`'s
        // reason. Switching to a table lands on the saw, and to a recording
        // on the patch's first: the chooser says what the oscillator *is*,
        // and what it reads is the next choice.
        "kind" => {
            if matches!(osc.source, SynthSource::Noise) {
                return false;
            }
            osc.source = match choice_index(value, SOURCE_KINDS.len()) {
                0 => match osc.source {
                    SynthSource::Table(_) | SynthSource::User(_) => osc.source,
                    _ => SynthSource::Table(WavetableId::Saw),
                },
                1 => match osc.source {
                    SynthSource::Sample(_) => osc.source,
                    _ => {
                        // The position becomes the start, and a table's
                        // frame carried over would start every note partway
                        // through the recording.
                        osc.position = 0.0;
                        SynthSource::Sample(0)
                    }
                },
                _ => SynthSource::String,
            };
        }
        "sample/loop" => {
            osc.sample.loop_mode = SampleLoop::ALL[choice_index(value, SampleLoop::ALL.len())];
        }
        "sample/loop_start" => osc.sample.loop_start = value,
        "sample/loop_end" => osc.sample.loop_end = value,
        "string/stiffness" => osc.string.stiffness = value,
        "string/damping" => osc.string.damping = value,
        "string/strike" => osc.string.strike = lerp(value, STRIKE_MIN, STRIKE_MAX),
        "string/decay" => {
            osc.string.decay_s = lerp_log(value, STRING_DECAY_MIN_S, STRING_DECAY_MAX_S)
        }
        _ => return false,
    }
    true
}

/// The three kinds of source an oscillator can be, in the order the `kind`
/// chooser offers them — and their names.
pub const SOURCE_KINDS: [&str; 3] = ["Table", "Sample", "String"];

/// Which position of [`SOURCE_KINDS`] a source is.
pub fn source_kind(source: SynthSource) -> usize {
    match source {
        SynthSource::Table(_) | SynthSource::User(_) | SynthSource::Noise => 0,
        SynthSource::Sample(_) => 1,
        SynthSource::String => 2,
    }
}

/// The travel of a string's strike knob, as a fraction of the string. Two
/// per cent is a hammer at the very end, which excites everything; a half
/// is the middle, which excites only the odd partials.
pub const STRIKE_MIN: f32 = 0.02;
pub const STRIKE_MAX: f32 = 0.5;
/// And of its decay, in seconds for middle C's fundamental.
pub const STRING_DECAY_MIN_S: f32 = 0.05;
pub const STRING_DECAY_MAX_S: f32 = 20.0;

/// How many positions the modulator chooser has: "none", plus the four layers
/// that could be later than layer 0.
pub const MODULATOR_CHOICES: usize = 5;

// -------------------------------------------------------------- the read ---

/// What `address` is worth right now, normalised — the other half of [`set`].
///
/// `None` for an address this patch has no such control for, which is the same
/// answer `set` gives by returning `false`.
pub fn value(patch: &Patch, address: &str) -> Option<f32> {
    match address {
        "patch/voice/polyphony" => Some(unlerp(
            f32::from(patch.voice_config.polyphony),
            1.0,
            MAX_POLYPHONY,
        )),
        "patch/voice/glide" => Some(unlerp(patch.voice_config.glide_time_s, 0.0, GLIDE_MAX_S)),
        "patch/voice/legato" => Some(bool_value(patch.voice_config.glide_legato_only)),
        "patch/voice/mode" => {
            let at = VOICE_MODES
                .iter()
                .position(|(m, _)| *m == patch.voice_config.retrigger)?;
            Some(choice_value(at, VOICE_MODES.len()))
        }
        "patch/voice/bend_range" => Some(unlerp(
            patch.voice_config.bend_range_semitones,
            0.0,
            BEND_MAX_SEMITONES,
        )),
        "patch/output" => Some(unlerp(patch.output_db, OUTPUT_MIN_DB, OUTPUT_MAX_DB)),
        "patch/quality" => {
            // Every layer carries the same one — see `set`. The first is the
            // patch's answer, and a patch with no layers has none.
            let current = patch.layers.first()?.playback.interpolation;
            let index = QUALITIES.iter().position(|(q, _)| *q == current)?;
            Some(choice_value(index, QUALITIES.len()))
        }
        _ => {
            if let Some((index, field)) = indexed(address, "patch/filter[") {
                let filter = patch.filters.get(index)?;
                return match field {
                    "enabled" => Some(bool_value(filter.enabled)),
                    "mode" => {
                        let at = FILTER_MODES.iter().position(|(m, _)| *m == filter.mode)?;
                        Some(choice_value(at, FILTER_MODES.len()))
                    }
                    "cutoff" => Some(unlerp_log(filter.cutoff_hz, CUTOFF_MIN_HZ, CUTOFF_MAX_HZ)),
                    "resonance" => Some(filter.resonance.clamp(0.0, 1.0)),
                    "slope" => {
                        let at = FilterSlope::ALL.iter().position(|s| *s == filter.slope)?;
                        Some(choice_value(at, FilterSlope::ALL.len()))
                    }
                    "model" => {
                        let at = FilterModel::ALL.iter().position(|m| *m == filter.model)?;
                        Some(choice_value(at, FilterModel::ALL.len()))
                    }
                    "drive" => Some(filter.drive.clamp(0.0, 1.0)),
                    "key_track" => Some(filter.key_track.clamp(0.0, 1.0)),
                    "character" => Some(filter.character.clamp(0.0, 1.0)),
                    _ => None,
                };
            }
            if let Some((index, field)) = indexed(address, "patch/env[") {
                let env = patch.envelopes.get(index)?;
                return match field {
                    "delay" => Some(unlerp_stage(env.delay_s)),
                    "attack" => Some(unlerp_stage(env.attack_s)),
                    "hold" => Some(unlerp_stage(env.hold_s)),
                    "decay" => Some(unlerp_stage(env.decay_s)),
                    "sustain" => Some(env.sustain_level.clamp(0.0, 1.0)),
                    "release" => Some(unlerp_stage(env.release_s)),
                    "attack_shape" => Some((env.attack_shape.clamp(-1.0, 1.0) + 1.0) / 2.0),
                    "decay_shape" => Some((env.decay_shape.clamp(-1.0, 1.0) + 1.0) / 2.0),
                    "release_shape" => Some((env.release_shape.clamp(-1.0, 1.0) + 1.0) / 2.0),
                    _ => None,
                };
            }
            if let Some((index, field)) = indexed(address, "patch/lfo[") {
                let lfo = patch.lfos.get(index)?;
                return match field {
                    "wave" => {
                        let at = LfoWave::ALL.iter().position(|w| *w == lfo.wave)?;
                        Some(choice_value(at, LfoWave::ALL.len()))
                    }
                    "rate" => Some(unlerp_log(lfo.rate_hz, LFO_MIN_HZ, LFO_MAX_HZ)),
                    "sync" => Some(bool_value(lfo.sync)),
                    "division" => {
                        let at = NoteDivision::ALL.iter().position(|d| *d == lfo.division)?;
                        Some(choice_value(at, NoteDivision::ALL.len()))
                    }
                    "depth" => Some(lfo.depth.clamp(0.0, 1.0)),
                    "delay" => Some(
                        (lfo.delay_s.max(0.0) / LFO_TIME_MAX_S)
                            .clamp(0.0, 1.0)
                            .cbrt(),
                    ),
                    "fade" => Some(
                        (lfo.fade_s.max(0.0) / LFO_TIME_MAX_S)
                            .clamp(0.0, 1.0)
                            .cbrt(),
                    ),
                    "phase" => Some(lfo.phase.clamp(0.0, 1.0)),
                    "mode" => {
                        let at = LfoMode::ALL.iter().position(|m| *m == lfo.mode)?;
                        Some(choice_value(at, LfoMode::ALL.len()))
                    }
                    "smooth" => Some(lfo.smooth.clamp(0.0, 1.0)),
                    _ => None,
                };
            }
            if let Some((index, field)) = indexed(address, "patch/mod[") {
                if field != "depth" {
                    return None;
                }
                let route = patch.mod_matrix.routes.get(index)?;
                return Some((route.depth.clamp(-1.0, 1.0) + 1.0) / 2.0);
            }
            if let Some((index, field)) = indexed(address, "patch/fx[") {
                let slot = patch.fx.get(index)?;
                if field == "enabled" {
                    return Some(bool_value(slot.enabled));
                }
                return slot.config.normalised(field);
            }
            if let Some(rest) = address.strip_prefix("patch/macro[")
                && let Some(number) = rest.strip_suffix(']')
                && let Ok(index) = number.parse::<usize>()
                && index < MACRO_COUNT
            {
                return Some(patch.macros[index].value.clamp(0.0, 1.0));
            }
            if let Some((index, field)) = indexed(address, "patch/layer[") {
                let layer = patch.layers.get(index)?;
                if let Some(field) = field.strip_prefix("synth/") {
                    let Source::Synth(osc) = &layer.source else {
                        return None;
                    };
                    return synth_value(osc, field);
                }
                return match field {
                    "gain" => Some(unlerp(layer.gain_db, GAIN_MIN_DB, GAIN_MAX_DB)),
                    "pan" => Some((layer.pan.clamp(-1.0, 1.0) + 1.0) / 2.0),
                    "shape" => match layer.source {
                        crate::patch::Source::Oscillator(kind) => {
                            let at = SHAPES.iter().position(|(k, _)| *k == kind)?;
                            Some(choice_value(at, SHAPES.len()))
                        }
                        _ => None,
                    },
                    "octave" => {
                        if !matches!(layer.source, crate::patch::Source::Oscillator(_)) {
                            return None;
                        }
                        let at = OCTAVES
                            .iter()
                            .position(|o| *o == octave_of(layer.root_key))?;
                        Some(choice_value(at, OCTAVES.len()))
                    }
                    "tune" => {
                        // Accepted on a synth layer as well as an oscillator
                        // one, because `set` is — and a read-out that
                        // disagreed with what it wrote is exactly the defect
                        // this module's docs say is not expressible.
                        if !matches!(
                            layer.source,
                            crate::patch::Source::Oscillator(_) | Source::Synth(_)
                        ) {
                            return None;
                        }
                        Some(unlerp(
                            layer.fine_tune_cents.clamp(-DETUNE_CENTS, DETUNE_CENTS),
                            -DETUNE_CENTS,
                            DETUNE_CENTS,
                        ))
                    }
                    _ => None,
                };
            }
            None
        }
    }
}

fn synth_value(osc: &fontelle_dsp::SynthOsc, field: &str) -> Option<f32> {
    match field {
        "kind" => {
            if matches!(osc.source, SynthSource::Noise) {
                return None;
            }
            Some(choice_value(source_kind(osc.source), SOURCE_KINDS.len()))
        }
        "sample/loop" => {
            let at = SampleLoop::ALL
                .iter()
                .position(|m| *m == osc.sample.loop_mode)?;
            Some(choice_value(at, SampleLoop::ALL.len()))
        }
        "sample/loop_start" => Some(osc.sample.loop_start.clamp(0.0, 1.0)),
        "sample/loop_end" => Some(osc.sample.loop_end.clamp(0.0, 1.0)),
        "string/stiffness" => Some(osc.string.stiffness.clamp(0.0, 1.0)),
        "string/damping" => Some(osc.string.damping.clamp(0.0, 1.0)),
        "string/strike" => Some(unlerp(osc.string.strike, STRIKE_MIN, STRIKE_MAX)),
        "string/decay" => Some(unlerp_log(
            osc.string
                .decay_s
                .clamp(STRING_DECAY_MIN_S, STRING_DECAY_MAX_S),
            STRING_DECAY_MIN_S,
            STRING_DECAY_MAX_S,
        )),
        "table" => match osc.source {
            SynthSource::Table(id) => {
                let at = WavetableId::ALL.iter().position(|t| *t == id)?;
                Some(choice_value(at, WavetableId::ALL.len()))
            }
            // A dropped sound is not a position in the bank's list, so the
            // chooser reads as nothing rather than as whichever table happens
            // to sit at index zero. The window names it from
            // `Patch::wavetables` instead.
            SynthSource::User(_)
            | SynthSource::Sample(_)
            | SynthSource::String
            | SynthSource::Noise => None,
        },
        "position" => Some(osc.position.clamp(0.0, 1.0)),
        "warp_mode" => {
            let at = WarpMode::ALL.iter().position(|w| *w == osc.warp)?;
            Some(choice_value(at, WarpMode::ALL.len()))
        }
        "warp" => Some(osc.warp_amount.clamp(0.0, 1.0)),
        "modulator" => Some(choice_value(
            osc.modulator
                .map_or(0, |m| usize::from(m).min(MODULATOR_CHOICES - 1)),
            MODULATOR_CHOICES,
        )),
        "semitones" => Some(unlerp(
            f32::from(osc.semitones),
            -SEMITONE_RANGE,
            SEMITONE_RANGE,
        )),
        "phase" => Some(osc.phase.clamp(0.0, 1.0)),
        "random_phase" => Some(bool_value(osc.random_phase)),
        "key_track" => Some(bool_value(osc.key_track)),
        "route" => {
            let at = FilterRoute::ALL
                .iter()
                .position(|r| *r == osc.filter_route)?;
            Some(choice_value(at, FilterRoute::ALL.len()))
        }
        "unison/voices" => Some(unlerp(f32::from(osc.unison.voices), 1.0, MAX_UNISON as f32)),
        "unison/detune" => Some(unlerp(
            osc.unison.detune_cents,
            0.0,
            UNISON_DETUNE_MAX_CENTS,
        )),
        "unison/blend" => Some(osc.unison.blend.clamp(0.0, 1.0)),
        "unison/width" => Some(osc.unison.width.clamp(0.0, 1.0)),
        "noise_colour" => Some(osc.noise_colour.clamp(0.0, 1.0)),
        _ => None,
    }
}

// --------------------------------------------------------------- tapers ---

/// Splits `prefix[N]/field` into `(N, field)`.
///
/// Its own function because getting it wrong silently writes to the wrong
/// filter, which is the kind of bug you hear rather than see.
fn indexed<'a>(address: &'a str, prefix: &str) -> Option<(usize, &'a str)> {
    let rest = address.strip_prefix(prefix)?;
    let (number, field) = rest.split_once("]/")?;
    Some((number.parse().ok()?, field))
}

pub fn lerp(t: f32, min: f32, max: f32) -> f32 {
    min + t.clamp(0.0, 1.0) * (max - min)
}

pub fn unlerp(value: f32, min: f32, max: f32) -> f32 {
    if (max - min).abs() < f32::EPSILON {
        return 0.0;
    }
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

/// A logarithmic taper, so an octave is the same distance everywhere on the
/// dial.
pub fn lerp_log(t: f32, min: f32, max: f32) -> f32 {
    (min.ln() + t.clamp(0.0, 1.0) * (max.ln() - min.ln())).exp()
}

pub fn unlerp_log(value: f32, min: f32, max: f32) -> f32 {
    if value <= 0.0 {
        return 0.0;
    }
    ((value.ln() - min.ln()) / (max.ln() - min.ln())).clamp(0.0, 1.0)
}

/// Envelope times, on a curve rather than a line: the first tenth of the dial
/// has to cover a click's attack and the last has to reach a pad's release.
pub fn lerp_stage(t: f32) -> f32 {
    t.clamp(0.0, 1.0).powi(3) * ENV_MAX_S
}

pub fn unlerp_stage(seconds: f32) -> f32 {
    (seconds.max(0.0) / ENV_MAX_S).clamp(0.0, 1.0).cbrt()
}

pub fn bool_value(on: bool) -> f32 {
    if on { 1.0 } else { 0.0 }
}

/// Where option `index` of `count` sits on a normalised dial. The endpoints are
/// the first and last option — [`choice_index`] is the reader this has to agree
/// with.
pub fn choice_value(index: usize, count: usize) -> f32 {
    if count < 2 {
        return 0.0;
    }
    index as f32 / (count - 1) as f32
}

pub fn choice_index(value: f32, count: usize) -> usize {
    if count < 2 {
        return 0;
    }
    let last = count - 1;
    ((value.clamp(0.0, 1.0) * last as f32).round() as usize).min(last)
}

/// The middle-C-relative root key an oscillator at `octave` plays from.
///
/// A **higher** root is a **lower** pitch, because the transposition an
/// oscillator gets is the one a sample gets — `key - root_key`. See
/// [`crate::OSC_ROOT_HZ`].
pub fn root_key_for(octave: i32) -> u8 {
    (60 - octave * 12).clamp(0, 127) as u8
}

/// The inverse, rounded to the nearest whole octave.
pub fn octave_of(root_key: u8) -> i32 {
    ((60 - i32::from(root_key)) as f32 / 12.0).round() as i32
}
