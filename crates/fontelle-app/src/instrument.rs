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
use fontelle_dsp::Oversampling;
use fontelle_types::ParamAddress;
use fontelle_ui::canvas::{InstrumentGroup, InstrumentParam, InstrumentView, ParamKind};

/// The channel's own level and placement, as a group.
///
/// Its own function because a channel playing a plugin has these two controls
/// as much as one playing a patch does — they belong to the channel, not to
/// what is on it (see `fontelle_model::Channel::gain_db`) — and two copies of
/// the same two addresses is one to forget.
pub fn channel_group(gain_db: f32, pan: f32) -> InstrumentGroup {
    InstrumentGroup {
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
    }
}

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
    GLIDE_MAX_S, MAX_POLYPHONY, OCTAVES, QUALITIES, SHAPES, bool_value, choice_index, choice_value,
    lerp, lerp_log, lerp_stage, octave_of, root_key_for, set, unlerp, unlerp_log, unlerp_stage,
    value,
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
    // Flopsynth has five oscillators, four envelopes, four LFOs, four macros,
    // a mod matrix and an effects chain — a hundred and fifty controls where
    // the general panel draws twenty. Drawing it here would mean this function
    // spending most of its length on one instrument, so it has its own.
    if fontelle_core::flopsynth::is_flopsynth(patch) {
        return describe_flopsynth(title, patch, gain_db, pan);
    }
    let mut groups = Vec::new();

    groups.push(channel_group(gain_db, pan));

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
                bool_value(voice.glide_mode == fontelle_core::GlideMode::Legato),
                on_off(voice.glide_mode == fontelle_core::GlideMode::Legato),
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
        keys: Vec::new(),
        key: None,
        title: title.to_string(),
        groups,
    }
}

// --------------------------------------------------------------- helpers ---

/// A chooser over [`Oversampling::ALL`] — the patch's, or one oscillator's.
fn oversampling_param(address: &str, label: &str, at: Oversampling) -> InstrumentParam {
    let index = Oversampling::ALL.iter().position(|q| *q == at).unwrap_or(0);
    param(
        address,
        label,
        choice_value(index, Oversampling::ALL.len()),
        at.label().to_string(),
        ParamKind::Choice(
            Oversampling::ALL
                .iter()
                .map(|q| q.label().to_string())
                .collect(),
        ),
    )
}

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

// ---------------------------------------------------------- hosted plugins

/// The most of a plugin's parameters this draws.
///
/// A plugin is entitled to expose thousands — a modular synth exposes one per
/// patch point — and a panel of thousands of knobs is not a panel. What it
/// draws is the first hundred and twenty-eight, in the order the plugin
/// declared them, which is the order its own editor would show them in.
///
/// The rest are not lost: they are still automatable and still saved, because
/// both of those go by the plugin's own parameter id and neither goes through
/// this list. Only the drawing stops.
pub const MAX_PLUGIN_PARAMS: usize = 128;

/// A panel for a plugin somebody else wrote (TDD §8.4).
///
/// The counterpart of [`describe`], and deliberately the same shape: an
/// [`InstrumentView`] of groups of normalised values with stable addresses.
/// That the panel needed no changes at all to draw a plugin is the strongest
/// evidence §8.2's parameter contract was the right one — a hosted plugin's
/// parameters differ from a built-in effect's in exactly one way, which is
/// that their names arrive at run time rather than living in the binary.
///
/// Grouped by the plugin's own `module` string, in the order the groups first
/// appear. A plugin that offers none gets one group called "Parameters",
/// which is what its parameters are.
///
/// `address_of` says how a parameter of *this* plugin is named — an insert's
/// and an instrument's differ, and neither is invented here (see
/// `fontelle_types::ParamTarget`). `display` is the plugin's own read-out for
/// a value: units are its business, and a host that guessed would put decibels
/// after a ratio.
pub fn describe_plugin(
    title: &str,
    params: &[fontelle_host::HostedParam],
    value_of: impl Fn(u32) -> Option<f64>,
    address_of: impl Fn(u32) -> ParamAddress,
    display: impl Fn(u32, f64) -> Option<String>,
) -> InstrumentView {
    let mut groups: Vec<InstrumentGroup> = Vec::new();
    for spec in params
        .iter()
        // A parameter the plugin says cannot be set is not a control. It is
        // still automatable and still saved — both go by its own id and
        // neither goes through this list — so only the drawing stops, which
        // is the same trade `MAX_PLUGIN_PARAMS` makes.
        .filter(|param| !param.hidden && !param.readonly)
        .take(MAX_PLUGIN_PARAMS)
    {
        let plain = value_of(spec.id).unwrap_or(spec.default);
        let drawn = InstrumentParam {
            address: address_of(spec.id),
            label: elide_label(&spec.name),
            value: spec.normalise(plain) as f32,
            display: display(spec.id, plain).unwrap_or_else(|| format_plain(plain, spec.stepped)),
            // A stepped parameter with two positions is a switch; anything
            // else is a knob, and a stepped one lands on its positions because
            // `HostedParam::plain` rounds. A `Choice` would need names for the
            // positions, and CLAP offers none without asking the plugin to
            // format each one — which is a main-thread call per position per
            // repaint.
            kind: if spec.steps() == Some(2) {
                ParamKind::Switch
            } else {
                ParamKind::Knob
            },
            automated: false,
        };
        let module = module_heading(&spec.module);
        match groups.iter_mut().find(|group| group.name == module) {
            Some(group) => group.params.push(drawn),
            None => groups.push(InstrumentGroup {
                name: module,
                params: vec![drawn],
            }),
        }
    }
    InstrumentView {
        title: title.to_string(),
        keys: Vec::new(),
        key: None,
        groups,
    }
}

/// The fallback read-out for a plugin that will not format its own values.
fn format_plain(value: f64, stepped: bool) -> String {
    if stepped {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.2}")
    }
}

/// What a group of a plugin's parameters is called on the panel.
///
/// CLAP's `module` is a **path** — "the plugin's grouping, `/`-separated" —
/// and plugins write it with separators: Surge XT's are `/Macros/` and
/// `/Global & FX/`, and drawing the string as it arrives put "/Macros/" over
/// the first row of knobs. The path is turned back into words, with the
/// segments spaced around a separator so a nested one still reads as nested
/// ("A / Osc 1"), and a plugin that offers no module at all gets "Parameters",
/// which is what its parameters are.
pub fn module_heading(module: &str) -> String {
    let words: Vec<&str> = module
        .split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect();
    if words.is_empty() {
        return "Parameters".to_string();
    }
    words.join(" \u{2044} ")
}

/// The most characters a control's caption may have before it is shortened.
///
/// The panel's cells are [`fontelle_ui::canvas::CELL_WIDTH`] wide, which is
/// about this many characters at the panel's own font. A plugin names its
/// parameters for its own editor, where there is room: "Polyphony Limit",
/// "Send FX 1 Return", "FX A1 Param 12".
const LABEL_CHARS: usize = 15;

/// `name`, shortened to fit a cell, with an ellipsis if it did not.
///
/// Cut with a mark rather than clipped by the renderer, because those are two
/// different sentences: "Polyphony Limi" reads as a name somebody misspelled,
/// and "Polyphony Limi\u{2026}" reads as a name that did not fit. The whole
/// name is still what the plugin knows it by — only the caption is shortened.
pub fn elide_label(name: &str) -> String {
    if name.chars().count() <= LABEL_CHARS {
        return name.to_string();
    }
    let kept: String = name.chars().take(LABEL_CHARS - 1).collect();
    format!("{}\u{2026}", kept.trim_end())
}

// ------------------------------------------------------------- Flopsynth ---

/// The panel for the built-in wavetable synthesiser
/// (`docs/flopsynth-plan.md` §8).
///
/// # Why it is the general grid and not (yet) the picture of a signal path
///
/// §8 asks for a bespoke canvas — cards laid out as the signal flows, a wave
/// picture per oscillator, a response curve per filter, draggable envelope
/// nodes, a modulation ring on every knob. That is Phase 4's work and it is a
/// lot of it.
///
/// What this is, is **every control reachable now**: one group per card the
/// bespoke window will draw, in the order it will draw them, using the same
/// addresses. So the synth is fully editable, fully automatable and fully on
/// the live wire from the day it ships, and the canvas replaces the *drawing*
/// rather than the plumbing.
///
/// Every address here is one [`fontelle_core::flopsynth::addresses`] lists,
/// which is what `realise`'s `param_nodes` map reads — so a knob on this panel
/// and a lane that can reach it are one list by construction (handoff §4).
pub fn describe_flopsynth(title: &str, patch: &Patch, gain_db: f32, pan: f32) -> InstrumentView {
    use fontelle_core::flopsynth::layer_role;
    use fontelle_core::patch_params::{
        BEND_MAX_SEMITONES, LFO_MAX_HZ, LFO_MIN_HZ, LFO_TIME_MAX_S, MODULATOR_CHOICES,
        OUTPUT_MAX_DB, OUTPUT_MIN_DB, SEMITONE_RANGE, UNISON_DETUNE_MAX_CENTS, VOICE_MODES,
    };
    use fontelle_core::patch_params::{
        SOURCE_KINDS, STRIKE_MAX, STRIKE_MIN, STRING_DECAY_MAX_S, STRING_DECAY_MIN_S, source_kind,
        unlerp_log,
    };
    use fontelle_dsp::{
        FilterModel, FilterRoute, FilterSlope, GRAIN_MAX_MS, GRAIN_MIN_MS, MAX_UNISON, SampleLoop,
        SynthSource, WarpMode, WavetableId,
    };
    use fontelle_types::{LfoWave, NoteDivision};

    let mut groups = vec![channel_group(gain_db, pan)];

    // --- Voice ---------------------------------------------------------
    let voice = &patch.voice_config;
    let mode = VOICE_MODES
        .iter()
        .position(|(m, _)| *m == voice.retrigger)
        .unwrap_or(0);
    groups.push(InstrumentGroup {
        name: "Voice".to_string(),
        params: vec![
            param(
                "patch/voice/mode",
                "mode",
                choice_value(mode, VOICE_MODES.len()),
                VOICE_MODES[mode].1.to_string(),
                ParamKind::Choice(VOICE_MODES.iter().map(|(_, n)| n.to_string()).collect()),
            ),
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
            // The glide's mode and shape (§4.6) and the velocity curve
            // (§4.2), each a chooser; the curve is the card's picture.
            {
                use fontelle_core::GlideMode;
                let at = GlideMode::ALL
                    .iter()
                    .position(|m| *m == voice.glide_mode)
                    .unwrap_or(0);
                param(
                    "patch/voice/glide_mode",
                    "when",
                    choice_value(at, GlideMode::ALL.len()),
                    voice.glide_mode.label().to_string(),
                    ParamKind::Choice(
                        GlideMode::ALL
                            .iter()
                            .map(|m| m.label().to_string())
                            .collect(),
                    ),
                )
            },
            {
                use fontelle_core::GlideCurve;
                let at = GlideCurve::ALL
                    .iter()
                    .position(|c| *c == voice.glide_curve)
                    .unwrap_or(0);
                param(
                    "patch/voice/glide_curve",
                    "shape",
                    choice_value(at, GlideCurve::ALL.len()),
                    voice.glide_curve.label().to_string(),
                    ParamKind::Choice(
                        GlideCurve::ALL
                            .iter()
                            .map(|c| c.label().to_string())
                            .collect(),
                    ),
                )
            },
            {
                use fontelle_core::VelocityCurve;
                param(
                    "patch/voice/velocity_curve",
                    "velocity",
                    choice_value(voice.velocity_curve.index(), VelocityCurve::ALL.len()),
                    voice.velocity_curve.label().to_string(),
                    ParamKind::Choice(
                        VelocityCurve::ALL
                            .iter()
                            .map(|c| c.label().to_string())
                            .collect(),
                    ),
                )
            },
            param(
                "patch/voice/bend_range",
                "bend",
                unlerp(voice.bend_range_semitones, 0.0, BEND_MAX_SEMITONES),
                format!("{:.0} st", voice.bend_range_semitones),
                ParamKind::Knob,
            ),
            param(
                "patch/output",
                "output",
                unlerp(patch.output_db, OUTPUT_MIN_DB, OUTPUT_MAX_DB),
                format!("{:+.1} dB", patch.output_db),
                ParamKind::Knob,
            ),
            // The patch's oversampling (`docs/flopsynth-next.md` §4.1):
            // what every oscillator without its own runs at, and the
            // ladder.
            oversampling_param("patch/oversampling", "oversample", patch.oversampling),
        ],
    });

    // --- The five oscillators, one card each ---------------------------
    for (index, layer) in patch.layers.iter().enumerate() {
        let fontelle_core::Source::Synth(osc) = &layer.source else {
            continue;
        };
        let role = layer_role(index);
        let mut params = Vec::new();
        let noise = matches!(osc.source, SynthSource::Noise);
        // What the position knob *is* on this kind of source: the frame of
        // a table, where a recording starts, how hard a string is struck.
        // Same address, same route, different word — see `SynthOsc::position`.
        let position_caption = match osc.source {
            SynthSource::Sample(_) | SynthSource::Spectral(_) => "start",
            SynthSource::String => "bright",
            _ => "pos",
        };

        if noise {
            params.push(param(
                &format!("patch/layer[{index}]/synth/noise_colour"),
                "colour",
                osc.noise_colour.clamp(0.0, 1.0),
                match osc.noise_colour {
                    c if c < 0.2 => "white".to_string(),
                    c if c < 0.7 => "pink".to_string(),
                    _ => "brown".to_string(),
                },
                ParamKind::Knob,
            ));
        } else {
            // The kind first: it decides what the rest of the card is.
            let kind = source_kind(osc.source);
            params.push(param(
                &format!("patch/layer[{index}]/synth/kind"),
                "kind",
                choice_value(kind, SOURCE_KINDS.len()),
                SOURCE_KINDS[kind].to_string(),
                ParamKind::Choice(SOURCE_KINDS.iter().map(|k| k.to_string()).collect()),
            ));
            if let SynthSource::Table(table) = osc.source {
                let at = WavetableId::ALL
                    .iter()
                    .position(|t| *t == table)
                    .unwrap_or(0);
                params.push(param(
                    &format!("patch/layer[{index}]/synth/table"),
                    "table",
                    choice_value(at, WavetableId::ALL.len()),
                    table.label().to_string(),
                    // Grouped by family in the bespoke window; a flat list
                    // here, in the same order, so the two never disagree
                    // about which position is which table (INVARIANT 7).
                    ParamKind::Choice(
                        WavetableId::ALL
                            .iter()
                            .map(|t| t.label().to_string())
                            .collect(),
                    ),
                ));
            }
            // A dropped table (`SynthSource::User`) has no chooser: it is not
            // in the bank's list, and its name is on its picture. It used
            // to fall through to the noise arm and draw a colour knob.
            params.push(param(
                &format!("patch/layer[{index}]/synth/position"),
                position_caption,
                osc.position.clamp(0.0, 1.0),
                format!("{:.0}%", osc.position.clamp(0.0, 1.0) * 100.0),
                ParamKind::Knob,
            ));
        }

        params.push(param(
            &format!("patch/layer[{index}]/gain"),
            "level",
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
        params.push(param(
            &format!("patch/layer[{index}]/pan"),
            "pan",
            (layer.pan.clamp(-1.0, 1.0) + 1.0) / 2.0,
            pan_display(layer.pan),
            ParamKind::Knob,
        ));

        match osc.source {
            SynthSource::Sample(sample_at) => {
                let at = SampleLoop::ALL
                    .iter()
                    .position(|m| *m == osc.sample.loop_mode)
                    .unwrap_or(0);
                params.push(param(
                    &format!("patch/layer[{index}]/synth/sample/loop"),
                    "loop",
                    choice_value(at, SampleLoop::ALL.len()),
                    osc.sample.loop_mode.label().to_string(),
                    ParamKind::Choice(
                        SampleLoop::ALL
                            .iter()
                            .map(|m| m.label().to_string())
                            .collect(),
                    ),
                ));
                if osc.sample.loop_mode == SampleLoop::Grains {
                    // A cloud has no loop: its two knobs stand where the
                    // loop points stood, so the card is the same size in
                    // every mode.
                    let grain = osc.sample.grain_ms.clamp(GRAIN_MIN_MS, GRAIN_MAX_MS);
                    params.push(param(
                        &format!("patch/layer[{index}]/synth/sample/grain"),
                        "grain",
                        unlerp_log(grain, GRAIN_MIN_MS, GRAIN_MAX_MS),
                        format!("{grain:.0} ms"),
                        ParamKind::Knob,
                    ));
                    params.push(param(
                        &format!("patch/layer[{index}]/synth/sample/spray"),
                        "spray",
                        osc.sample.spray.clamp(0.0, 1.0),
                        format!("{:.0}%", osc.sample.spray.clamp(0.0, 1.0) * 100.0),
                        ParamKind::Knob,
                    ));
                } else {
                    params.push(param(
                        &format!("patch/layer[{index}]/synth/sample/loop_start"),
                        "loop in",
                        osc.sample.loop_start.clamp(0.0, 1.0),
                        format!("{:.0}%", osc.sample.loop_start.clamp(0.0, 1.0) * 100.0),
                        ParamKind::Knob,
                    ));
                    params.push(param(
                        &format!("patch/layer[{index}]/synth/sample/loop_end"),
                        "loop out",
                        osc.sample.loop_end.clamp(0.0, 1.0),
                        format!("{:.0}%", osc.sample.loop_end.clamp(0.0, 1.0) * 100.0),
                        ParamKind::Knob,
                    ));
                }
                // The zone chooser, on a recording with zones to choose
                // between: *any* — the zone whose range holds the key —
                // then each by name, which on a kit is the hit's. One zone
                // is no choice, and the card leaves the chooser out.
                if let Some(sample) = patch.samples.get(usize::from(sample_at))
                    && sample.zones.len() > 1
                {
                    let choices = 1 + sample.zones.len();
                    let chosen = osc
                        .sample
                        .zone
                        .map_or(0, |z| usize::from(z) + 1)
                        .min(choices - 1);
                    let names: Vec<String> = std::iter::once("any".to_string())
                        .chain(sample.zones.iter().map(|zone| zone.label()))
                        .collect();
                    params.push(param(
                        &format!("patch/layer[{index}]/synth/sample/zone"),
                        "zone",
                        choice_value(chosen, choices),
                        names[chosen].clone(),
                        ParamKind::Choice(names),
                    ));
                }
            }
            // A spectral read (§4.3): the zone chooser, as a plain read has
            // it; the rest is the position and the warp.
            SynthSource::Spectral(sample_at) => {
                if let Some(sample) = patch.samples.get(usize::from(sample_at))
                    && sample.zones.len() > 1
                {
                    let choices = 1 + sample.zones.len();
                    let chosen = osc
                        .sample
                        .zone
                        .map_or(0, |z| usize::from(z) + 1)
                        .min(choices - 1);
                    let names: Vec<String> = std::iter::once("any".to_string())
                        .chain(sample.zones.iter().map(|zone| zone.label()))
                        .collect();
                    params.push(param(
                        &format!("patch/layer[{index}]/synth/sample/zone"),
                        "zone",
                        choice_value(chosen, choices),
                        names[chosen].clone(),
                        ParamKind::Choice(names),
                    ));
                }
            }
            SynthSource::String => {
                let string = &osc.string;
                params.push(param(
                    &format!("patch/layer[{index}]/synth/string/stiffness"),
                    "stiff",
                    string.stiffness.clamp(0.0, 1.0),
                    format!("{:.0}%", string.stiffness.clamp(0.0, 1.0) * 100.0),
                    ParamKind::Knob,
                ));
                params.push(param(
                    &format!("patch/layer[{index}]/synth/string/damping"),
                    "damp",
                    string.damping.clamp(0.0, 1.0),
                    format!("{:.0}%", string.damping.clamp(0.0, 1.0) * 100.0),
                    ParamKind::Knob,
                ));
                params.push(param(
                    &format!("patch/layer[{index}]/synth/string/strike"),
                    "strike",
                    unlerp(string.strike, STRIKE_MIN, STRIKE_MAX),
                    // Where along the string, as the fraction every
                    // textbook writes it: "1/8" is a piano.
                    format!("1/{:.0}", 1.0 / string.strike.clamp(STRIKE_MIN, STRIKE_MAX)),
                    ParamKind::Knob,
                ));
                params.push(param(
                    &format!("patch/layer[{index}]/synth/string/decay"),
                    "ring",
                    unlerp_log(
                        string.decay_s.clamp(STRING_DECAY_MIN_S, STRING_DECAY_MAX_S),
                        STRING_DECAY_MIN_S,
                        STRING_DECAY_MAX_S,
                    ),
                    seconds(string.decay_s),
                    ParamKind::Knob,
                ));
            }
            _ => {}
        }

        // The warp reads a table's phase or a recording's head; a string
        // has neither, so its card leaves the warp out. The start phase is
        // a table's alone: a recording starts where its start knob says and
        // a string starts from rest.
        let warps = !noise && !matches!(osc.source, SynthSource::String);
        let has_phase =
            !noise && !matches!(osc.source, SynthSource::String | SynthSource::Sample(_));
        if warps {
            let warp = WarpMode::ALL
                .iter()
                .position(|m| *m == osc.warp)
                .unwrap_or(0);
            params.push(param(
                &format!("patch/layer[{index}]/synth/warp_mode"),
                "warp",
                choice_value(warp, WarpMode::ALL.len()),
                osc.warp.label().to_string(),
                ParamKind::Choice(
                    WarpMode::ALL
                        .iter()
                        .map(|m| m.label().to_string())
                        .collect(),
                ),
            ));
            params.push(param(
                &format!("patch/layer[{index}]/synth/warp"),
                "amount",
                osc.warp_amount.clamp(0.0, 1.0),
                format!("{:.0}%", osc.warp_amount.clamp(0.0, 1.0) * 100.0),
                ParamKind::Knob,
            ));
            // Only the layers *after* this one can be its modulator, which is
            // what makes the voice's backwards walk correct — see
            // `Voice::render_performing`.
            let choices: Vec<String> = std::iter::once("none".to_string())
                .chain((1..MODULATOR_CHOICES).map(|i| layer_role(i).label().to_string()))
                .collect();
            let at = osc
                .modulator
                .map_or(0, |m| usize::from(m).min(choices.len() - 1));
            params.push(param(
                &format!("patch/layer[{index}]/synth/modulator"),
                "mod from",
                choice_value(at, MODULATOR_CHOICES),
                choices[at].clone(),
                ParamKind::Choice(choices),
            ));
        }
        if !noise {
            params.push(param(
                &format!("patch/layer[{index}]/synth/unison/voices"),
                "unison",
                unlerp(f32::from(osc.unison.voices), 1.0, MAX_UNISON as f32),
                format!("{}", osc.unison.voices),
                ParamKind::Knob,
            ));
            params.push(param(
                &format!("patch/layer[{index}]/synth/unison/detune"),
                "detune",
                unlerp(osc.unison.detune_cents, 0.0, UNISON_DETUNE_MAX_CENTS),
                format!("{:.0} c", osc.unison.detune_cents),
                ParamKind::Knob,
            ));
            params.push(param(
                &format!("patch/layer[{index}]/synth/unison/blend"),
                "blend",
                osc.unison.blend.clamp(0.0, 1.0),
                format!("{:.0}%", osc.unison.blend.clamp(0.0, 1.0) * 100.0),
                ParamKind::Knob,
            ));
            params.push(param(
                &format!("patch/layer[{index}]/synth/unison/width"),
                "width",
                osc.unison.width.clamp(0.0, 1.0),
                format!("{:.0}%", osc.unison.width.clamp(0.0, 1.0) * 100.0),
                ParamKind::Knob,
            ));
            // The stack's mode and spread (§4.3), two choosers.
            {
                use fontelle_core::patch_params::UNISON_MODES;
                let at = UNISON_MODES
                    .iter()
                    .position(|(m, _)| *m == osc.unison.mode)
                    .unwrap_or(0);
                params.push(param(
                    &format!("patch/layer[{index}]/synth/unison/mode"),
                    "stack",
                    choice_value(at, UNISON_MODES.len()),
                    UNISON_MODES[at].1.to_string(),
                    ParamKind::Choice(UNISON_MODES.iter().map(|(_, n)| n.to_string()).collect()),
                ));
                let at = fontelle_dsp::UnisonSpread::ALL
                    .iter()
                    .position(|s| *s == osc.unison.spread)
                    .unwrap_or(0);
                params.push(param(
                    &format!("patch/layer[{index}]/synth/unison/spread"),
                    "spread",
                    choice_value(at, fontelle_dsp::UnisonSpread::ALL.len()),
                    osc.unison.spread.label().to_string(),
                    ParamKind::Choice(
                        fontelle_dsp::UnisonSpread::ALL
                            .iter()
                            .map(|s| s.label().to_string())
                            .collect(),
                    ),
                ));
            }
        }
        if has_phase {
            params.push(param(
                &format!("patch/layer[{index}]/synth/phase"),
                "phase",
                osc.phase.clamp(0.0, 1.0),
                format!("{:.0}\u{b0}", osc.phase.clamp(0.0, 1.0) * 360.0),
                ParamKind::Knob,
            ));
            params.push(param(
                &format!("patch/layer[{index}]/synth/random_phase"),
                "random",
                bool_value(osc.random_phase),
                on_off(osc.random_phase),
                ParamKind::Switch,
            ));
        }

        params.push(param(
            &format!("patch/layer[{index}]/synth/semitones"),
            "semis",
            unlerp(f32::from(osc.semitones), -SEMITONE_RANGE, SEMITONE_RANGE),
            format!("{:+} st", osc.semitones),
            ParamKind::Knob,
        ));
        params.push(param(
            &format!("patch/layer[{index}]/tune"),
            "fine",
            unlerp(
                layer.fine_tune_cents.clamp(-DETUNE_CENTS, DETUNE_CENTS),
                -DETUNE_CENTS,
                DETUNE_CENTS,
            ),
            format!("{:+.0} c", layer.fine_tune_cents),
            ParamKind::Knob,
        ));
        params.push(param(
            &format!("patch/layer[{index}]/synth/key_track"),
            "key",
            bool_value(osc.key_track),
            on_off(osc.key_track),
            ParamKind::Switch,
        ));
        let route = FilterRoute::ALL
            .iter()
            .position(|r| *r == osc.filter_route)
            .unwrap_or(0);
        params.push(param(
            &format!("patch/layer[{index}]/synth/route"),
            "route",
            choice_value(route, FilterRoute::ALL.len()),
            osc.filter_route.label().to_string(),
            ParamKind::Choice(
                FilterRoute::ALL
                    .iter()
                    .map(|r| r.label().to_string())
                    .collect(),
            ),
        ));
        // Last, so nothing above it moved when it arrived. The noise has no
        // read to oversample.
        if !noise {
            params.push(oversampling_param(
                &format!("patch/layer[{index}]/synth/quality"),
                "quality",
                osc.quality,
            ));
        }

        groups.push(InstrumentGroup {
            name: role.label().to_string(),
            params,
        });
    }

    // --- The two filters -----------------------------------------------
    for (index, filter) in patch.filters.iter().enumerate() {
        let model = FilterModel::ALL
            .iter()
            .position(|m| *m == filter.model)
            .unwrap_or(0);
        let mode = FILTER_MODES
            .iter()
            .position(|(m, _)| *m == filter.mode)
            .unwrap_or(0);
        let slope = FilterSlope::ALL
            .iter()
            .position(|s| *s == filter.slope)
            .unwrap_or(0);
        let mut params = vec![
            param(
                &format!("patch/filter[{index}]/enabled"),
                "on",
                bool_value(filter.enabled),
                on_off(filter.enabled),
                ParamKind::Switch,
            ),
            param(
                &format!("patch/filter[{index}]/model"),
                "model",
                choice_value(model, FilterModel::ALL.len()),
                filter.model.label().to_string(),
                ParamKind::Choice(
                    FilterModel::ALL
                        .iter()
                        .map(|m| m.label().to_string())
                        .collect(),
                ),
            ),
            param(
                &format!("patch/filter[{index}]/mode"),
                "shape",
                choice_value(mode, FILTER_MODES.len()),
                FILTER_MODES[mode].1.to_string(),
                ParamKind::Choice(FILTER_MODES.iter().map(|(_, n)| n.to_string()).collect()),
            ),
            param(
                &format!("patch/filter[{index}]/slope"),
                "slope",
                choice_value(slope, FilterSlope::ALL.len()),
                filter.slope.label().to_string(),
                ParamKind::Choice(
                    FilterSlope::ALL
                        .iter()
                        .map(|s| s.label().to_string())
                        .collect(),
                ),
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
            param(
                &format!("patch/filter[{index}]/drive"),
                "drive",
                filter.drive.clamp(0.0, 1.0),
                format!("{:.0}%", filter.drive.clamp(0.0, 1.0) * 100.0),
                ParamKind::Knob,
            ),
            param(
                &format!("patch/filter[{index}]/key_track"),
                "key trk",
                filter.key_track.clamp(0.0, 1.0),
                format!("{:.0}%", filter.key_track.clamp(0.0, 1.0) * 100.0),
                ParamKind::Knob,
            ),
        ];
        // The character knob's caption is the *model's* — and a model that has
        // no use for it does not get a knob that does nothing.
        if let Some(caption) = filter.model.character_label() {
            params.push(param(
                &format!("patch/filter[{index}]/character"),
                caption,
                filter.character.clamp(0.0, 1.0),
                format!("{:.0}%", filter.character.clamp(0.0, 1.0) * 100.0),
                ParamKind::Knob,
            ));
        }
        groups.push(InstrumentGroup {
            name: format!("Filter {}", index + 1),
            params,
        });
    }

    // --- Four envelopes ------------------------------------------------
    for (index, env) in patch
        .envelopes
        .iter()
        .enumerate()
        .take(fontelle_core::MAX_MOD_ENVELOPES + 1)
    {
        let shape = |field: &str, label: &str, value: f32| {
            param(
                &format!("patch/env[{index}]/{field}"),
                label,
                (value.clamp(-1.0, 1.0) + 1.0) / 2.0,
                format!("{value:+.2}"),
                ParamKind::Knob,
            )
        };
        groups.push(InstrumentGroup {
            // Envelope 1 is the amp envelope everywhere in this program; 2 is
            // the one the Init patch routes to the filter.
            name: match index {
                0 => "ENV 1 \u{b7} amp".to_string(),
                1 => "ENV 2 \u{b7} filter".to_string(),
                n => format!("ENV {}", n + 1),
            },
            params: vec![
                stage(&format!("patch/env[{index}]/delay"), "delay", env.delay_s),
                stage(
                    &format!("patch/env[{index}]/attack"),
                    "attack",
                    env.attack_s,
                ),
                stage(&format!("patch/env[{index}]/hold"), "hold", env.hold_s),
                stage(&format!("patch/env[{index}]/decay"), "decay", env.decay_s),
                param(
                    &format!("patch/env[{index}]/sustain"),
                    "sustain",
                    env.sustain_level.clamp(0.0, 1.0),
                    format!("{:.0}%", env.sustain_level.clamp(0.0, 1.0) * 100.0),
                    ParamKind::Knob,
                ),
                stage(
                    &format!("patch/env[{index}]/release"),
                    "release",
                    env.release_s,
                ),
                shape("attack_shape", "a shape", env.attack_shape),
                shape("decay_shape", "d shape", env.decay_shape),
                shape("release_shape", "r shape", env.release_shape),
                // The loop (`docs/flopsynth-next.md` §3.4): a pair of
                // stages, or off.
                {
                    use fontelle_core::patch_params::ENV_LOOPS;
                    let at = ENV_LOOPS
                        .iter()
                        .position(|(stages, _)| *stages == env.loop_stages)
                        .unwrap_or(0);
                    param(
                        &format!("patch/env[{index}]/loop"),
                        "loop",
                        choice_value(at, ENV_LOOPS.len()),
                        ENV_LOOPS[at].1.to_string(),
                        ParamKind::Choice(ENV_LOOPS.iter().map(|(_, n)| n.to_string()).collect()),
                    )
                },
            ],
        });
    }

    // --- The LFOs, eight since phase 3 --------------------------------
    for (index, lfo) in patch.lfos.iter().enumerate().take(fontelle_core::MAX_LFOS) {
        let wave = LfoWave::ALL
            .iter()
            .position(|w| *w == lfo.wave)
            .unwrap_or(0);
        let division = NoteDivision::ALL
            .iter()
            .position(|d| *d == lfo.division)
            .unwrap_or(0);
        let mode = fontelle_core::LfoMode::ALL
            .iter()
            .position(|m| *m == lfo.mode)
            .unwrap_or(0);
        let cubic = |value: f32| (value.max(0.0) / LFO_TIME_MAX_S).clamp(0.0, 1.0).cbrt();
        groups.push(InstrumentGroup {
            name: format!("LFO {}", index + 1),
            params: vec![
                param(
                    &format!("patch/lfo[{index}]/wave"),
                    "wave",
                    choice_value(wave, LfoWave::ALL.len()),
                    lfo.wave.label().to_string(),
                    ParamKind::Choice(LfoWave::ALL.iter().map(|w| w.label().to_string()).collect()),
                ),
                param(
                    &format!("patch/lfo[{index}]/sync"),
                    "sync",
                    bool_value(lfo.sync),
                    on_off(lfo.sync),
                    ParamKind::Switch,
                ),
                // Both are always drawn, and the read-out says which one is in
                // force: a control that vanished when a switch moved is a
                // control somebody has to hunt for.
                param(
                    &format!("patch/lfo[{index}]/rate"),
                    "rate",
                    unlerp_log(lfo.rate_hz, LFO_MIN_HZ, LFO_MAX_HZ),
                    if lfo.sync {
                        "(synced)".to_string()
                    } else {
                        format!("{:.2} Hz", lfo.rate_hz)
                    },
                    ParamKind::Knob,
                ),
                param(
                    &format!("patch/lfo[{index}]/division"),
                    "division",
                    choice_value(division, NoteDivision::ALL.len()),
                    lfo.division.label().to_string(),
                    ParamKind::Choice(
                        NoteDivision::ALL
                            .iter()
                            .map(|d| d.label().to_string())
                            .collect(),
                    ),
                ),
                param(
                    &format!("patch/lfo[{index}]/mode"),
                    "mode",
                    choice_value(mode, fontelle_core::LfoMode::ALL.len()),
                    lfo.mode.label().to_string(),
                    ParamKind::Choice(
                        fontelle_core::LfoMode::ALL
                            .iter()
                            .map(|m| m.label().to_string())
                            .collect(),
                    ),
                ),
                param(
                    &format!("patch/lfo[{index}]/depth"),
                    "depth",
                    lfo.depth.clamp(0.0, 1.0),
                    format!("{:.0}%", lfo.depth.clamp(0.0, 1.0) * 100.0),
                    ParamKind::Knob,
                ),
                param(
                    &format!("patch/lfo[{index}]/delay"),
                    "delay",
                    cubic(lfo.delay_s),
                    seconds(lfo.delay_s),
                    ParamKind::Knob,
                ),
                param(
                    &format!("patch/lfo[{index}]/fade"),
                    "fade",
                    cubic(lfo.fade_s),
                    seconds(lfo.fade_s),
                    ParamKind::Knob,
                ),
                param(
                    &format!("patch/lfo[{index}]/phase"),
                    "phase",
                    lfo.phase.clamp(0.0, 1.0),
                    format!("{:.0}\u{b0}", lfo.phase.clamp(0.0, 1.0) * 360.0),
                    ParamKind::Knob,
                ),
                param(
                    &format!("patch/lfo[{index}]/smooth"),
                    "smooth",
                    lfo.smooth.clamp(0.0, 1.0),
                    format!("{:.0}%", lfo.smooth.clamp(0.0, 1.0) * 100.0),
                    ParamKind::Knob,
                ),
                // The shape editor's own (`docs/flopsynth-next.md` §3.4):
                // whether a drawn shape plays, the grid it snaps to, and
                // whether it is read smooth or as steps.
                param(
                    &format!("patch/lfo[{index}]/draw"),
                    "draw",
                    bool_value(lfo.shape.is_some()),
                    on_off(lfo.shape.is_some()),
                    ParamKind::Switch,
                ),
                {
                    use fontelle_core::patch_params::LFO_GRIDS;
                    let grid = lfo.shape.as_ref().map_or(8, |shape| shape.grid);
                    let at = LFO_GRIDS.iter().position(|g| *g == grid).unwrap_or(0);
                    let names: Vec<String> = LFO_GRIDS
                        .iter()
                        .map(|g| {
                            if *g == 0 {
                                "off".to_string()
                            } else {
                                g.to_string()
                            }
                        })
                        .collect();
                    param(
                        &format!("patch/lfo[{index}]/grid"),
                        "grid",
                        choice_value(at, LFO_GRIDS.len()),
                        names[at].clone(),
                        ParamKind::Choice(names),
                    )
                },
                {
                    let step = lfo
                        .shape
                        .as_ref()
                        .is_some_and(|shape| shape.mode == fontelle_types::LfoShapeMode::Step);
                    param(
                        &format!("patch/lfo[{index}]/shape_mode"),
                        "read",
                        bool_value(step),
                        if step { "step" } else { "smooth" }.to_string(),
                        ParamKind::Choice(vec!["smooth".to_string(), "step".to_string()]),
                    )
                },
            ],
        });
    }

    // --- The §4.2 generators: two sequencers, the chaos, the walk -------
    //
    // Each edited in the inspector like an LFO. A sequencer's sixteen steps
    // are its picture, dragged, not sixteen knobs — the `step[n]` addresses
    // are in `flopsynth::addresses` for automation and the drag, and only
    // the clock and the length are controls here.
    for (index, seq) in patch.sequencers.iter().enumerate() {
        use fontelle_core::mod_sources::SEQ_STEPS;
        let division = NoteDivision::ALL
            .iter()
            .position(|d| *d == seq.division)
            .unwrap_or(0);
        let length = usize::from(seq.length.clamp(1, SEQ_STEPS as u8));
        groups.push(InstrumentGroup {
            name: format!("SEQ {}", index + 1),
            params: vec![
                param(
                    &format!("patch/seq[{index}]/length"),
                    "steps",
                    choice_value(length - 1, SEQ_STEPS),
                    length.to_string(),
                    ParamKind::Choice((1..=SEQ_STEPS).map(|n| n.to_string()).collect()),
                ),
                param(
                    &format!("patch/seq[{index}]/sync"),
                    "sync",
                    bool_value(seq.sync),
                    on_off(seq.sync),
                    ParamKind::Switch,
                ),
                param(
                    &format!("patch/seq[{index}]/rate"),
                    "rate",
                    unlerp_log(seq.rate_hz, LFO_MIN_HZ, LFO_MAX_HZ),
                    if seq.sync {
                        "(synced)".to_string()
                    } else {
                        format!("{:.2} Hz", seq.rate_hz)
                    },
                    ParamKind::Knob,
                ),
                param(
                    &format!("patch/seq[{index}]/division"),
                    "division",
                    choice_value(division, NoteDivision::ALL.len()),
                    seq.division.label().to_string(),
                    ParamKind::Choice(
                        NoteDivision::ALL
                            .iter()
                            .map(|d| d.label().to_string())
                            .collect(),
                    ),
                ),
                param(
                    &format!("patch/seq[{index}]/smooth"),
                    "smooth",
                    seq.smooth.clamp(0.0, 1.0),
                    format!("{:.0}%", seq.smooth.clamp(0.0, 1.0) * 100.0),
                    ParamKind::Knob,
                ),
            ],
        });
    }
    {
        use fontelle_core::patch_params::{CHAOS_MAX_HZ, CHAOS_MIN_HZ};
        groups.push(InstrumentGroup {
            name: "Chaos".to_string(),
            params: vec![param(
                "patch/chaos/rate",
                "rate",
                unlerp_log(patch.chaos.rate_hz, CHAOS_MIN_HZ, CHAOS_MAX_HZ),
                format!("{:.2} Hz", patch.chaos.rate_hz),
                ParamKind::Knob,
            )],
        });
        groups.push(InstrumentGroup {
            name: "Walk".to_string(),
            params: vec![
                param(
                    "patch/walk/rate",
                    "rate",
                    unlerp_log(patch.walk.rate_hz, LFO_MIN_HZ, LFO_MAX_HZ),
                    format!("{:.2} Hz", patch.walk.rate_hz),
                    ParamKind::Knob,
                ),
                param(
                    "patch/walk/smooth",
                    "smooth",
                    patch.walk.smooth.clamp(0.0, 1.0),
                    format!("{:.0}%", patch.walk.smooth.clamp(0.0, 1.0) * 100.0),
                    ParamKind::Knob,
                ),
            ],
        });
    }

    // --- The macros, captioned with their own names --------------------
    groups.push(InstrumentGroup {
        name: "Macros".to_string(),
        params: (0..fontelle_core::MACRO_COUNT)
            .map(|index| {
                let knob = &patch.macros[index];
                // A macro's caption **is** its name, which is the whole of
                // what makes a preset playable from one knob. A macro nobody
                // has named is still a knob, so it gets its number.
                let caption = if knob.name.is_empty() {
                    format!("macro {}", index + 1)
                } else {
                    knob.name.clone()
                };
                param(
                    &format!("patch/macro[{index}]"),
                    &caption,
                    knob.value.clamp(0.0, 1.0),
                    format!("{:.0}%", knob.value.clamp(0.0, 1.0) * 100.0),
                    ParamKind::Knob,
                )
            })
            .collect(),
    });

    // --- The matrix's depths -------------------------------------------
    //
    // A route's **depth** is a parameter and everything else about it is
    // structure (§2.3), so a depth is a knob here and adding a route is not.
    // The caption is the route itself, so a panel of eight of these is
    // readable rather than eight knobs called "depth".
    if !patch.mod_matrix.routes.is_empty() {
        let destinations = fontelle_core::flopsynth::destinations(patch);
        let sources = fontelle_core::flopsynth::sources(patch);
        groups.push(InstrumentGroup {
            name: format!("Modulation ({})", patch.mod_matrix.routes.len()),
            params: patch
                .mod_matrix
                .routes
                .iter()
                .enumerate()
                .map(|(index, route)| {
                    let name_of = |want: &dyn Fn() -> Option<String>| {
                        want().unwrap_or_else(|| "?".to_string())
                    };
                    let source = name_of(&|| {
                        sources
                            .iter()
                            .find(|(s, _)| *s == route.source)
                            .map(|(_, l)| l.clone())
                    });
                    let destination = name_of(&|| {
                        destinations
                            .iter()
                            .find(|(d, _)| *d == route.destination)
                            .map(|(_, l)| l.clone())
                    });
                    param(
                        &format!("patch/mod[{index}]/depth"),
                        &format!("{source} \u{2192} {destination}"),
                        (route.depth.clamp(-1.0, 1.0) + 1.0) / 2.0,
                        format!("{:+.2}", route.depth),
                        ParamKind::Knob,
                    )
                })
                .collect(),
        });
    }

    // --- The instrument's own effects ----------------------------------
    for (index, slot) in patch.fx.iter().enumerate() {
        let mut params = vec![param(
            &format!("patch/fx[{index}]/enabled"),
            "on",
            bool_value(slot.enabled),
            on_off(slot.enabled),
            ParamKind::Switch,
        )];
        // The effect's own `ParamSpec` list, which is already the list
        // automation works from — built by the same function the effect
        // window uses, so a chorus's mode is a chooser that says its names
        // here too, and a new effect's knobs are automatable the day they
        // exist with nothing said twice.
        params.extend(fontelle_ui::canvas::effect_params(&slot.config, |id| {
            fontelle_types::ParamAddress::new(format!("patch/fx[{index}]/{id}"))
        }));
        groups.push(InstrumentGroup {
            name: format!("FX {} \u{b7} {}", index + 1, slot.config.kind().label()),
            params,
        });
    }

    InstrumentView {
        // The preset bar is the *system's* (§P.7) and not this panel's: the
        // chip row here is the effects' one, and a Flopsynth's hundred and
        // twenty-eight presets are not a row of chips.
        keys: Vec::new(),
        key: None,
        title: title.to_string(),
        groups,
    }
}
