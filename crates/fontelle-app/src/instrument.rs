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

use fontelle_core::Patch;
use fontelle_types::ParamAddress;
use fontelle_ui::canvas::{InstrumentGroup, InstrumentParam, InstrumentView, ParamKind};

/// The two parameters that live on the **channel** rather than in its patch —
/// its level and its placement.
///
/// Re-exported from the crate that draws the panel rather than spelled again
/// here: the window has to recognise them too (a right-click on either makes an
/// automation lane), and one address written down twice is one to find and move.
///
/// They are called `mixer/*` for the reason INVARIANT 7 gives — an address
/// never changes once assigned — and not because they reach the mixer, which
/// they no longer do. See `fontelle_model::Channel::gain_db`.
pub use fontelle_ui::canvas::{MIXER_GAIN, MIXER_PAN};

/// Everything the two halves of the table share: the ranges each control is
/// drawn over, the lists the choosers step through, and the tapers between a
/// dial's fraction and a value in hertz or seconds.
///
/// **They live in `fontelle-core`**, beside the `Patch` they describe, because
/// the audio thread needs them too — see
/// [`fontelle_core::patch_params`], which is the write half of this file and
/// the reason *"every knob is automatable"* is true by construction rather
/// than by two lists agreeing.
pub use fontelle_core::patch_params::{
    CUTOFF_MAX_HZ, CUTOFF_MIN_HZ, DETUNE_CENTS, FILTER_MODES, GAIN_MAX_DB, GAIN_MIN_DB,
    GLIDE_MAX_S, MAX_POLYPHONY, OCTAVES, QUALITIES, SHAPES, bool_value, choice_index,
    choice_value, lerp, lerp_log, lerp_stage, octave_of, root_key_for, set, unlerp, unlerp_log,
    unlerp_stage, value,
};

/// Every address on this patch's panel that belongs to the **patch** rather
/// than to the channel around it.
///
/// Taken from [`describe`] rather than from a list written beside it, because
/// the two would drift: a knob added to the panel and forgotten here would be
/// a knob you can right-click and cannot automate, and the lane would be made,
/// drawn, saved and silent. See `realise`, which is the caller.
pub fn patch_addresses(patch: &Patch) -> Vec<String> {
    describe("", patch, 0.0, 0.0)
        .groups
        .iter()
        .flat_map(|group| group.params.iter())
        .map(|param| param.address.as_str().to_string())
        .filter(|address| address.starts_with("patch/"))
        .collect()
}

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

    // The layers, which is where a multi-sample soundfont's balance lives —
    // and where the built-in synth's three oscillators live, since an
    // oscillator *is* a layer (`Source::Oscillator`). Four at most: a panel of
    // sixteen layers is a list, not an editor, and the list is what the
    // sampler editor panel becomes later.
    let shown = patch.layers.len().min(4);
    if shown > 0 {
        // An all-oscillator patch is the built-in synth, and calling its
        // section "Layers" would be technically right and useless. A patch
        // that is some of each keeps the general heading.
        let all_oscillators = patch
            .layers
            .iter()
            .all(|l| matches!(l.source, fontelle_core::Source::Oscillator(_)));
        let mut params = Vec::new();
        for index in 0..shown {
            let layer = &patch.layers[index];
            let name = |field: &str| {
                if all_oscillators {
                    format!("osc {} {field}", index + 1)
                } else {
                    format!("L{} {field}", index + 1)
                }
            };
            // An oscillator has a shape and a tuning; a sample has neither,
            // because its shape and its pitch are what was recorded.
            if let fontelle_core::Source::Oscillator(kind) = layer.source {
                let shape = SHAPES.iter().position(|(k, _)| *k == kind).unwrap_or(0);
                params.push(param(
                    &format!("patch/layer[{index}]/shape"),
                    &name("shape"),
                    choice_value(shape, SHAPES.len()),
                    SHAPES[shape].1.to_string(),
                    ParamKind::Choice(SHAPES.iter().map(|(_, n)| n.to_string()).collect()),
                ));
            }
            params.push(param(
                &format!("patch/layer[{index}]/gain"),
                &name("level"),
                unlerp(layer.gain_db, GAIN_MIN_DB, GAIN_MAX_DB),
                if layer.gain_db <= GAIN_MIN_DB {
                    // The bottom of the travel is *off*, and saying "-60.0 dB"
                    // instead makes somebody wonder whether they can hear it.
                    "off".to_string()
                } else {
                    format!("{:+.1} dB", layer.gain_db)
                },
                ParamKind::Knob,
            ));
            if matches!(layer.source, fontelle_core::Source::Oscillator(_)) {
                // Octaves as a chooser rather than a knob: there are five of
                // them and a knob that steps through five values is a knob
                // that is hard to land on the one you want. A **higher** root
                // plays lower — see `fontelle_core::OSC_ROOT_HZ`.
                let octave = octave_of(layer.root_key);
                let index_of = OCTAVES
                    .iter()
                    .position(|o| *o == octave)
                    .unwrap_or(OCTAVES.len() / 2);
                params.push(param(
                    &format!("patch/layer[{index}]/octave"),
                    &name("octave"),
                    choice_value(index_of, OCTAVES.len()),
                    match OCTAVES[index_of] {
                        0 => "0".to_string(),
                        n => format!("{n:+}"),
                    },
                    ParamKind::Choice(
                        OCTAVES
                            .iter()
                            .map(|n| match n {
                                0 => "0".to_string(),
                                n => format!("{n:+}"),
                            })
                            .collect(),
                    ),
                ));
                params.push(param(
                    &format!("patch/layer[{index}]/tune"),
                    &name("tune"),
                    unlerp(
                        layer.fine_tune_cents.clamp(-DETUNE_CENTS, DETUNE_CENTS),
                        -DETUNE_CENTS,
                        DETUNE_CENTS,
                    ),
                    format!("{:+.0} c", layer.fine_tune_cents),
                    ParamKind::Knob,
                ));
            }
            params.push(param(
                &format!("patch/layer[{index}]/pan"),
                &name("pan"),
                (layer.pan.clamp(-1.0, 1.0) + 1.0) / 2.0,
                pan_display(layer.pan),
                ParamKind::Knob,
            ));
        }
        groups.push(InstrumentGroup {
            name: match (all_oscillators, patch.layers.len()) {
                (true, _) => "Oscillators".to_string(),
                (false, n) if n > shown => format!("Layers (first {shown} of {n})"),
                _ => "Layers".to_string(),
            },
            params,
        });
    }

    InstrumentView {
        presets: Vec::new(),
        keys: Vec::new(),
        key: None,
        title: title.to_string(),
        groups,
    }
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
        // Filled in by `InstrumentView::mark_automated`, which the session
        // calls once it has the set of lanes in hand.
        automated: false,
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








fn on_off(on: bool) -> String {
    if on { "on" } else { "off" }.to_string()
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
