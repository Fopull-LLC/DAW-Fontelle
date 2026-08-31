//! The instrument editor's parameter map: what a control on the panel means to
//! a `Patch` (TDD §7.2, §8.2).
//!
//! `fontelle-ui` draws a list of normalised values with stable addresses and
//! knows nothing about a patch; the mapping lives here, in the one layer
//! allowed to see both. Every address is written down exactly once, in
//! [`describe`] — the read and the write both go through the same table, so a
//! knob whose read-out disagrees with what it wrote is not expressible.
//!
//! **Addresses are INVARIANT 7's.** They never change once assigned. An address
//! this build does not recognise is ignored rather than refused, so a project
//! naming a parameter a later build dropped still opens.

use fontelle_core::{Patch, PlaybackConfig};
use fontelle_dsp::{Interpolation, SvfMode};
use fontelle_types::ParamAddress;
use fontelle_ui::canvas::{InstrumentGroup, InstrumentParam, InstrumentView, ParamKind};

/// The parameters that live on the **mixer track** rather than on the patch, so
/// the session knows to send those somewhere else.
pub const MIXER_GAIN: &str = "mixer/gain";
pub const MIXER_PAN: &str = "mixer/pan";

/// The loudest and quietest a channel fader goes, in dB.
pub const GAIN_MIN_DB: f32 = -60.0;
pub const GAIN_MAX_DB: f32 = 12.0;

/// The filter modes the panel offers, in the order the chip steps through them.
const FILTER_MODES: [(SvfMode, &str); 7] = [
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
const QUALITIES: [(Option<Interpolation>, &str); 5] = [
    (None, "Session"),
    (Some(Interpolation::Draft), "Draft"),
    (Some(Interpolation::Normal), "Normal"),
    (Some(Interpolation::High), "High"),
    (Some(Interpolation::Ultra), "Ultra"),
];

/// A cutoff runs 20 Hz to 20 kHz, and it runs **logarithmically**: a linear
/// cutoff knob spends nine tenths of its travel above 2 kHz, where almost
/// nothing musically interesting happens.
const CUTOFF_MIN_HZ: f32 = 20.0;
const CUTOFF_MAX_HZ: f32 = 20_000.0;

/// The longest an envelope stage may be set to here. Ten seconds is a pad's
/// release; beyond that a knob is unusable for everything shorter.
const ENV_MAX_S: f32 = 10.0;

/// And the longest a glide.
const GLIDE_MAX_S: f32 = 2.0;

/// The most voices a patch may be given. §7.4's limit.
const MAX_POLYPHONY: f32 = 256.0;

// -------------------------------------------------------------- the view ---

/// Everything the panel shows for one channel.
///
/// `gain_db` and `pan` come from the channel's mixer track: the window has no
/// mixer panel yet, and a sampler you cannot balance against another one is
/// half an instrument.
pub fn describe(title: &str, patch: &Patch, gain_db: f32, pan: f32) -> InstrumentView {
    let mut groups = Vec::new();

    groups.push(InstrumentGroup {
        name: "Channel".to_string(),
        params: vec![
            param(
                MIXER_GAIN,
                "volume",
                unlerp(gain_db, GAIN_MIN_DB, GAIN_MAX_DB),
                format!("{gain_db:+.1} dB"),
                ParamKind::Knob,
            ),
            param(
                MIXER_PAN,
                "pan",
                (pan + 1.0) / 2.0,
                pan_display(pan),
                ParamKind::Knob,
            ),
        ],
    });

    let voice = &patch.voice_config;
    groups.push(InstrumentGroup {
        name: "Voice".to_string(),
        params: vec![
            param(
                "patch/voice/polyphony",
                "voices",
                unlerp(f32::from(voice.polyphony), 1.0, MAX_POLYPHONY),
                format!("{}", voice.polyphony),
                ParamKind::Knob,
            ),
            param(
                "patch/voice/glide",
                "glide",
                unlerp(voice.glide_time_s, 0.0, GLIDE_MAX_S),
                seconds(voice.glide_time_s),
                ParamKind::Knob,
            ),
            param(
                "patch/voice/legato",
                "legato",
                bool_value(voice.glide_legato_only),
                on_off(voice.glide_legato_only),
                ParamKind::Switch,
            ),
            {
                // One control for every layer: a patch whose layers disagree
                // about interpolation is not something this panel can draw, and
                // §7.6's per-layer pin is a deliberate choice made per *patch*
                // far more often than per layer.
                let current = patch
                    .layers
                    .first()
                    .map(|l| l.playback.interpolation)
                    .unwrap_or(None);
                let index = QUALITIES
                    .iter()
                    .position(|(q, _)| *q == current)
                    .unwrap_or(0);
                param(
                    "patch/quality",
                    "quality",
                    choice_value(index, QUALITIES.len()),
                    QUALITIES[index].1.to_string(),
                    ParamKind::Choice(QUALITIES.iter().map(|(_, n)| n.to_string()).collect()),
                )
            },
        ],
    });

    for (index, filter) in patch.filters.iter().enumerate() {
        let mode = FILTER_MODES
            .iter()
            .position(|(m, _)| *m == filter.mode)
            .unwrap_or(0);
        groups.push(InstrumentGroup {
            name: format!("Filter {}", index + 1),
            params: vec![
                param(
                    &format!("patch/filter[{index}]/enabled"),
                    "on",
                    bool_value(filter.enabled),
                    on_off(filter.enabled),
                    ParamKind::Switch,
                ),
                param(
                    &format!("patch/filter[{index}]/mode"),
                    "mode",
                    choice_value(mode, FILTER_MODES.len()),
                    FILTER_MODES[mode].1.to_string(),
                    ParamKind::Choice(FILTER_MODES.iter().map(|(_, n)| n.to_string()).collect()),
                ),
                param(
                    &format!("patch/filter[{index}]/cutoff"),
                    "cutoff",
                    unlerp_log(filter.cutoff_hz, CUTOFF_MIN_HZ, CUTOFF_MAX_HZ),
                    hertz(filter.cutoff_hz),
                    ParamKind::Knob,
                ),
                param(
                    &format!("patch/filter[{index}]/resonance"),
                    "res",
                    filter.resonance.clamp(0.0, 1.0),
                    format!("{:.2}", filter.resonance),
                    ParamKind::Knob,
                ),
            ],
        });
    }

    // The amp envelope is the first one, by the same convention the importer
    // and the mod matrix use.
    if let Some(env) = patch.envelopes.first() {
        groups.push(InstrumentGroup {
            name: "Amp envelope".to_string(),
            params: vec![
                stage("patch/env[0]/delay", "delay", env.delay_s),
                stage("patch/env[0]/attack", "attack", env.attack_s),
                stage("patch/env[0]/hold", "hold", env.hold_s),
                stage("patch/env[0]/decay", "decay", env.decay_s),
                param(
                    "patch/env[0]/sustain",
                    "sustain",
                    env.sustain_level.clamp(0.0, 1.0),
                    format!("{:.0}%", env.sustain_level.clamp(0.0, 1.0) * 100.0),
                    ParamKind::Knob,
                ),
                stage("patch/env[0]/release", "release", env.release_s),
            ],
        });
    }

    // The layers, which is where a multi-sample soundfont's balance lives. Four
    // at most: a panel of sixteen layers is a list, not an editor, and the list
    // is what the sampler editor panel becomes later.
    let shown = patch.layers.len().min(4);
    if shown > 0 {
        let mut params = Vec::new();
        for index in 0..shown {
            let layer = &patch.layers[index];
            params.push(param(
                &format!("patch/layer[{index}]/gain"),
                &format!("L{} gain", index + 1),
                unlerp(layer.gain_db, GAIN_MIN_DB, GAIN_MAX_DB),
                format!("{:+.1} dB", layer.gain_db),
                ParamKind::Knob,
            ));
            params.push(param(
                &format!("patch/layer[{index}]/pan"),
                &format!("L{} pan", index + 1),
                (layer.pan.clamp(-1.0, 1.0) + 1.0) / 2.0,
                pan_display(layer.pan),
                ParamKind::Knob,
            ));
        }
        groups.push(InstrumentGroup {
            name: match patch.layers.len() {
                n if n > shown => format!("Layers (first {shown} of {n})"),
                _ => "Layers".to_string(),
            },
            params,
        });
    }

    InstrumentView {
        title: title.to_string(),
        groups,
    }
}

// ------------------------------------------------------------- the write ---

/// Applies one control to `patch`. Returns whether anything changed.
///
/// An address that names nothing is ignored — see the module's own docs.
pub fn set(patch: &mut Patch, address: &ParamAddress, value: f32) -> bool {
    let value = value.clamp(0.0, 1.0);
    let address = address.as_str();

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
            // Every layer, for the reason `describe` gives.
            for layer in &mut patch.layers {
                layer.playback = PlaybackConfig {
                    interpolation: quality,
                    ..layer.playback
                };
            }
            true
        }
        _ => {
            if let Some(rest) = indexed(address, "patch/filter[") {
                return set_filter(patch, rest.0, rest.1, value);
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
    match field {
        "gain" => layer.gain_db = lerp(value, GAIN_MIN_DB, GAIN_MAX_DB),
        "pan" => layer.pan = value * 2.0 - 1.0,
        _ => return false,
    }
    true
}

/// Splits `prefix[N]/field` into `(N, field)`.
///
/// Its own function because getting it wrong silently writes to the wrong
/// filter, which is the kind of bug you hear rather than see.
fn indexed<'a>(address: &'a str, prefix: &str) -> Option<(usize, &'a str)> {
    let rest = address.strip_prefix(prefix)?;
    let (number, field) = rest.split_once("]/")?;
    Some((number.parse().ok()?, field))
}

// --------------------------------------------------------------- helpers ---

fn param(
    address: &str,
    label: &str,
    value: f32,
    display: String,
    kind: ParamKind,
) -> InstrumentParam {
    InstrumentParam {
        address: ParamAddress::new(address),
        label: label.to_string(),
        value: value.clamp(0.0, 1.0),
        display,
        kind,
    }
}

fn stage(address: &str, label: &str, seconds_value: f32) -> InstrumentParam {
    param(
        address,
        label,
        unlerp_stage(seconds_value),
        seconds(seconds_value),
        ParamKind::Knob,
    )
}

fn lerp(t: f32, min: f32, max: f32) -> f32 {
    min + t.clamp(0.0, 1.0) * (max - min)
}

fn unlerp(value: f32, min: f32, max: f32) -> f32 {
    if (max - min).abs() < f32::EPSILON {
        return 0.0;
    }
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

/// A logarithmic taper, so an octave is the same distance everywhere on the
/// dial.
fn lerp_log(t: f32, min: f32, max: f32) -> f32 {
    (min.ln() + t.clamp(0.0, 1.0) * (max.ln() - min.ln())).exp()
}

fn unlerp_log(value: f32, min: f32, max: f32) -> f32 {
    if value <= 0.0 {
        return 0.0;
    }
    ((value.ln() - min.ln()) / (max.ln() - min.ln())).clamp(0.0, 1.0)
}

/// Envelope times, on a curve rather than a line: the first tenth of the dial
/// has to cover a click's attack and the last has to reach a pad's release.
fn lerp_stage(t: f32) -> f32 {
    t.clamp(0.0, 1.0).powi(3) * ENV_MAX_S
}

fn unlerp_stage(seconds_value: f32) -> f32 {
    (seconds_value.max(0.0) / ENV_MAX_S).clamp(0.0, 1.0).cbrt()
}

fn bool_value(on: bool) -> f32 {
    if on { 1.0 } else { 0.0 }
}

fn on_off(on: bool) -> String {
    if on { "on" } else { "off" }.to_string()
}

/// Where option `index` of `count` sits on a normalised dial. The endpoints are
/// the first and last option — see `fontelle_ui::canvas::choice_index`, which
/// is the reader this has to agree with.
fn choice_value(index: usize, count: usize) -> f32 {
    if count < 2 {
        return 0.0;
    }
    index as f32 / (count - 1) as f32
}

fn choice_index(value: f32, count: usize) -> usize {
    if count < 2 {
        return 0;
    }
    let last = count - 1;
    ((value.clamp(0.0, 1.0) * last as f32).round() as usize).min(last)
}

fn seconds(value: f32) -> String {
    if value < 0.001 {
        "0 ms".to_string()
    } else if value < 1.0 {
        format!("{:.0} ms", value * 1000.0)
    } else {
        format!("{value:.2} s")
    }
}

fn hertz(value: f32) -> String {
    if value >= 1000.0 {
        format!("{:.2} kHz", value / 1000.0)
    } else {
        format!("{value:.0} Hz")
    }
}

fn pan_display(pan: f32) -> String {
    let pan = pan.clamp(-1.0, 1.0);
    match (pan * 100.0).round() as i32 {
        0 => "centre".to_string(),
        n if n < 0 => format!("{}L", -n),
        n => format!("{n}R"),
    }
}
