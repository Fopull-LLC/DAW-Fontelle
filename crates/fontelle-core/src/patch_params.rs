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

use fontelle_dsp::{Interpolation, OscKind, SvfMode};

use crate::patch::Patch;
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
            false
        }
    }
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
        _ => return false,
    }
    true
}

fn set_layer(patch: &mut Patch, index: usize, field: &str, value: f32) -> bool {
    let Some(layer) = patch.layers.get_mut(index) else {
        return false;
    };
    let is_oscillator = matches!(layer.source, crate::patch::Source::Oscillator(_));
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
            if !is_oscillator {
                return false;
            }
            layer.fine_tune_cents = lerp(value, -DETUNE_CENTS, DETUNE_CENTS);
        }
        _ => return false,
    }
    true
}

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
                    _ => None,
                };
            }
            if let Some((index, field)) = indexed(address, "patch/layer[") {
                let layer = patch.layers.get(index)?;
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
                        let at = OCTAVES.iter().position(|o| *o == octave_of(layer.root_key))?;
                        Some(choice_value(at, OCTAVES.len()))
                    }
                    "tune" => {
                        if !matches!(layer.source, crate::patch::Source::Oscillator(_)) {
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
