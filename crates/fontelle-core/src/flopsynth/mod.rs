//! Flopsynth: the built-in wavetable synthesiser (`docs/flopsynth-plan.md`).
//!
//! > *"a new built in synthesizer plugin. this will be our main synth for the
//! > daw kind of like how fl studio has flex ... should be an advanced
//! > synthesizer inspired by the likes of omnisphere and Serum."* — Ty,
//! > 2026-09-06
//!
//! # It is not a special case
//!
//! The drum machine's lesson, taken again: a kit is an ordinary [`Patch`], so
//! save, load, automation, the key map, the mixer and undo never had to be
//! told it exists. Flopsynth is the same — an ordinary `Patch` whose layers
//! carry [`Source::Synth`]. There is no `Flopsynth` type, no second document,
//! no parallel save path. What is in this module is a **convention** and the
//! three functions that read it.
//!
//! # The convention
//!
//! Five layers in a fixed order: A, B, C, Sub, Noise ([`LayerRole`]). The
//! window relies on it; the voice does not. A patch with a sampled layer
//! appended is still a valid patch and still plays, which is precisely the
//! seam the Omnisphere-style hybrids of the plan's §12 arrive through — and
//! why the roles are a `layer_role(index)` function rather than a field on
//! the patch that could disagree with what is actually there.

use fontelle_dsp::{
    EnvelopeConfig, EnvelopeCurve, FilterModel, FilterRoute, FilterSlope, Oversampling, SvfMode,
    SynthOsc, SynthSource, WavetableId,
};
use fontelle_types::{LfoWave, NoteDivision};

use crate::mod_matrix::{Curve, ModDest, ModMatrix, ModRoute, ModSource};
use crate::patch::{FilterSlot, Layer, Lfo, LfoMode, MACRO_COUNT, Patch, SILENT_DB, Source};
use crate::playback::PlaybackConfig;
use crate::voice::VoiceConfig;

pub mod presets;

/// What each of a Flopsynth patch's five layers is for.
///
/// A convention the window draws from, **not** a rule the voice enforces —
/// see the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerRole {
    /// The three full oscillators: table, position, warp, unison, the lot.
    OscA,
    OscB,
    OscC,
    /// A single-voice oscillator on a short list of tables, usually an octave
    /// or two down. Nothing about it is special except the controls the window
    /// bothers to draw for it.
    Sub,
    Noise,
    /// A sixth layer or beyond — a sampled one appended to a Flopsynth patch,
    /// which is legal and plays. The window draws no card for it yet (§12).
    Extra,
}

impl LayerRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::OscA => "OSC A",
            Self::OscB => "OSC B",
            Self::OscC => "OSC C",
            Self::Sub => "SUB",
            Self::Noise => "NOISE",
            Self::Extra => "LAYER",
        }
    }

    /// Whether this role's card draws the full set of oscillator controls.
    /// The sub and the noise draw a handful; the three oscillators draw
    /// everything.
    pub fn is_full_oscillator(self) -> bool {
        matches!(self, Self::OscA | Self::OscB | Self::OscC)
    }
}

/// The five roles, in the order a Flopsynth patch's layers carry them.
pub const ROLES: [LayerRole; 5] = [
    LayerRole::OscA,
    LayerRole::OscB,
    LayerRole::OscC,
    LayerRole::Sub,
    LayerRole::Noise,
];

/// What layer `index` is for.
pub fn layer_role(index: usize) -> LayerRole {
    ROLES.get(index).copied().unwrap_or(LayerRole::Extra)
}

/// Whether this patch is one Flopsynth's window should be drawn for.
///
/// **Any** `Source::Synth` layer, not five of them in the right order: a patch
/// somebody has taken a layer off is still their patch, and a window that
/// vanished when they did would be a worse answer than one that draws four
/// cards.
/// The effect kinds a patch's own chain may hold, in the order the
/// `+ effect` list offers them (`docs/flopsynth-plan.md` §3.9).
///
/// **Zero-latency kinds only** (§2.2): the chain runs inside the instrument's
/// node, and an instrument with latency is the one case the graph does not
/// line up — so the gate, which looks ahead, is not here. The order is the
/// plan's: what a synth preset reaches for first, first.
pub const PATCH_FX_KINDS: [fontelle_types::EffectKind; 8] = [
    fontelle_types::EffectKind::Chorus,
    fontelle_types::EffectKind::Delay,
    fontelle_types::EffectKind::Reverb,
    fontelle_types::EffectKind::Filter,
    fontelle_types::EffectKind::Eq,
    fontelle_types::EffectKind::Distortion,
    fontelle_types::EffectKind::Bitcrush,
    fontelle_types::EffectKind::Compressor,
];

pub fn is_flopsynth(patch: &Patch) -> bool {
    patch
        .layers
        .iter()
        .any(|layer| matches!(layer.source, Source::Synth(_)))
}

/// A layer over every key and velocity, carrying `osc`.
fn synth_layer(osc: SynthOsc, gain_db: f32) -> Layer {
    Layer {
        source: Source::Synth(osc),
        // Every key, always: an oscillator has no recorded range to run out
        // of, and an instrument with holes in the keyboard would be a strange
        // thing to hand somebody.
        key_range: (0, 127),
        vel_range: (0, 127),
        // Middle C, so a note plays its own pitch — see `crate::OSC_ROOT_HZ`.
        // Flopsynth transposes with `SynthOsc::semitones` rather than by
        // moving the root, because a semitone knob that read "root key 72"
        // would be a knob nobody could set on purpose.
        root_key: 60,
        fine_tune_cents: 0.0,
        playback: PlaybackConfig::default(),
        gain_db,
        pan: 0.0,
    }
}

/// **The blank Flopsynth**: one saw, one open filter, an amp envelope with a
/// little shape on it, and four of everything else waiting.
///
/// Every choice here answers the same question the drum machine's default kit
/// and `Patch::basic_synth` answer — *what does somebody hear the instant they
/// put this on a channel, and what is the first knob they will reach for?*
///
/// - **Only oscillator A is up**, at −12 dB. Three oscillators at full level
///   is a default that clips on the first chord; the other four sit at
///   [`SILENT_DB`] with everything else about them set up so that turning the
///   level knob is the *only* step to hearing them.
/// - **Filter 1 is wide open.** A default patch that is already filtered is
///   one whose cutoff knob only ever brightens by undoing a choice nobody
///   made. A little resonance, so that closing it does something audible
///   immediately.
/// - **Envelope 2 is already routed to the cutoff, at depth zero.** The first
///   thing anybody turns up on a synthesiser is the filter envelope, and a
///   depth knob that is already there and reads "0.0" is a knob; a matrix row
///   somebody has to know to add is a manual.
/// - **A short attack and release**, not zero: a hard-gated saw clicks at both
///   ends, and "why does it tick" is not a first impression worth giving.
/// - **Polyphony 32, not 64.** Sixteen voices of three eight-voice unison
///   stacks is already a lot of oscillators, and the window says the number.
pub fn flopsynth_init() -> Patch {
    let osc_a = SynthOsc {
        source: SynthSource::Table(WavetableId::Saw),
        filter_route: FilterRoute::Serial,
        ..SynthOsc::default()
    };
    let osc_b = SynthOsc {
        source: SynthSource::Table(WavetableId::AnalogMorph),
        position: 0.5,
        filter_route: FilterRoute::Serial,
        ..SynthOsc::default()
    };
    let osc_c = SynthOsc {
        source: SynthSource::Table(WavetableId::Sawstack),
        filter_route: FilterRoute::Serial,
        ..SynthOsc::default()
    };
    let sub = SynthOsc {
        source: SynthSource::Table(WavetableId::SubSine),
        semitones: -12,
        // Around the filter: a sub whose bottom end disappears when the cutoff
        // closes is a sub that fights the patch instead of holding it up.
        filter_route: FilterRoute::Bypass,
        ..SynthOsc::default()
    };
    let noise = SynthOsc {
        source: SynthSource::Noise,
        // Noise has no pitch to track, and one that followed the keyboard
        // would be a filter sweep nobody asked for.
        key_track: false,
        filter_route: FilterRoute::Serial,
        ..SynthOsc::default()
    };

    // The amp envelope. Linear with shapes rather than `Decibel`: a stage time
    // that means "the time to travel 100 dB" is right for SF2 and confusing
    // under a shape knob — see `EnvelopeConfig::attack_shape`.
    let amp = EnvelopeConfig {
        attack_s: 0.005,
        sustain_level: 1.0,
        release_s: 0.15,
        curve: EnvelopeCurve::Linear,
        // −0.6 falls fast and then tails away, which is what an ear expects of
        // a decay and what the `Decibel` curve was giving before shapes
        // existed.
        decay_shape: -0.6,
        release_shape: -0.6,
        ..Default::default()
    };
    // The filter envelope, and the shape every further slot opens with.
    let filter_env = crate::patch::envelope_at_rest();

    let mut patch = Patch {
        wavetables: Vec::new(),
        samples: Vec::new(),
        oversampling: Oversampling::Off,
        layers: vec![
            synth_layer(osc_a, -12.0),
            synth_layer(osc_b, SILENT_DB),
            synth_layer(osc_c, SILENT_DB),
            synth_layer(sub, SILENT_DB),
            synth_layer(noise, SILENT_DB),
        ],
        filters: [
            FilterSlot {
                mode: SvfMode::Lowpass,
                cutoff_hz: 20_000.0,
                resonance: 0.2,
                enabled: true,
                slope: FilterSlope::Db24,
                model: FilterModel::Clean,
                ..Default::default()
            },
            FilterSlot {
                mode: SvfMode::Highpass,
                cutoff_hz: 20.0,
                resonance: 0.7,
                enabled: false,
                ..Default::default()
            },
        ],
        // Four of each here, as the bank's rows were written; the rest of
        // the six envelopes and eight LFOs (§4.2) are filled below by
        // `fill_modulator_slots`, the same as a row read from disk gets.
        envelopes: vec![amp, filter_env, filter_env, filter_env],
        lfos: vec![
            Lfo {
                rate_hz: 5.0,
                wave: LfoWave::Sine,
                ..Default::default()
            },
            Lfo {
                wave: LfoWave::Sine,
                sync: true,
                division: NoteDivision::Quarter,
                mode: LfoMode::Free,
                ..Default::default()
            },
            Lfo {
                rate_hz: 1.0,
                ..Default::default()
            },
            Lfo {
                rate_hz: 1.0,
                ..Default::default()
            },
        ],
        mod_matrix: ModMatrix {
            routes: vec![ModRoute {
                source: ModSource::Envelope(1),
                destination: ModDest::FilterCutoff(0),
                // Zero, so the route is *there* and does nothing until
                // somebody turns it up — see this function's own docs.
                depth: 0.0,
                curve: Curve::Linear,
                via: None,
                invert: false,
                bypass: false,
            }],
        },
        voice_config: VoiceConfig {
            polyphony: 32,
            bend_range_semitones: 2.0,
            ..VoiceConfig::default()
        },
        fx: Vec::new(),
        macros: Default::default(),
        output_db: 0.0,
    };
    patch.fill_modulator_slots();
    patch
}

/// Every address on a Flopsynth patch's window, in the order the panel offers
/// them.
///
/// **This is the list automation works from.** `instrument::patch_addresses`
/// dispatches to it and `realise`'s `param_nodes` map reads that, so a control
/// on the window and a lane that can reach it are the same list by
/// construction rather than by two lists agreeing (handoff §4). A knob added
/// to the window and forgotten here would be a knob you can right-click and
/// cannot automate, and the lane would be made, drawn, saved and silent.
pub fn addresses(patch: &Patch) -> Vec<String> {
    let mut out = vec![
        "patch/voice/polyphony".to_string(),
        "patch/voice/glide".to_string(),
        "patch/voice/legato".to_string(),
        "patch/voice/mode".to_string(),
        "patch/voice/bend_range".to_string(),
        "patch/output".to_string(),
        "patch/quality".to_string(),
        "patch/oversampling".to_string(),
    ];

    for (index, layer) in patch.layers.iter().enumerate() {
        out.push(format!("patch/layer[{index}]/gain"));
        out.push(format!("patch/layer[{index}]/pan"));
        // The **layer's** fine tune, which every kind of layer has: a
        // Flopsynth oscillator transposes in whole semitones with
        // `synth/semitones` and detunes in cents with this, the same as any
        // other layer. `patch_params::set` accepts it on a synth layer for
        // exactly this reason.
        out.push(format!("patch/layer[{index}]/tune"));
        let Source::Synth(osc) = &layer.source else {
            // A sampled or oscillator layer appended to a Flopsynth patch is
            // legal (§12) and keeps the addresses its own kind has.
            out.push(format!("patch/layer[{index}]/octave"));
            continue;
        };
        let noise = matches!(osc.source, SynthSource::Noise);
        if noise {
            out.push(format!("patch/layer[{index}]/synth/noise_colour"));
        } else {
            // What kind of source it is, then the controls that kind has: a
            // table's chooser, a recording's loop, a string's string. The
            // position is every kind's — its frame, its start, its
            // brightness — see `SynthOsc::position`.
            out.push(format!("patch/layer[{index}]/synth/kind"));
            match osc.source {
                SynthSource::Table(_) | SynthSource::User(_) => {
                    out.push(format!("patch/layer[{index}]/synth/table"));
                }
                SynthSource::Sample(_) => {
                    for field in ["loop", "loop_start", "loop_end", "grain", "spray", "zone"] {
                        out.push(format!("patch/layer[{index}]/synth/sample/{field}"));
                    }
                }
                SynthSource::String => {
                    for field in ["stiffness", "damping", "strike", "decay"] {
                        out.push(format!("patch/layer[{index}]/synth/string/{field}"));
                    }
                }
                SynthSource::Noise => {}
            }
            out.push(format!("patch/layer[{index}]/synth/position"));
            out.push(format!("patch/layer[{index}]/synth/warp_mode"));
            out.push(format!("patch/layer[{index}]/synth/warp"));
            out.push(format!("patch/layer[{index}]/synth/modulator"));
            out.push(format!("patch/layer[{index}]/synth/unison/voices"));
            out.push(format!("patch/layer[{index}]/synth/unison/detune"));
            out.push(format!("patch/layer[{index}]/synth/unison/blend"));
            out.push(format!("patch/layer[{index}]/synth/unison/width"));
            out.push(format!("patch/layer[{index}]/synth/phase"));
            out.push(format!("patch/layer[{index}]/synth/random_phase"));
        }
        out.push(format!("patch/layer[{index}]/synth/semitones"));
        out.push(format!("patch/layer[{index}]/synth/key_track"));
        out.push(format!("patch/layer[{index}]/synth/route"));
        // Last, so nothing above it moved when it arrived (§4.1). The
        // noise has no read to oversample.
        if !noise {
            out.push(format!("patch/layer[{index}]/synth/quality"));
        }
    }

    for index in 0..patch.filters.len() {
        for field in [
            "enabled",
            "mode",
            "cutoff",
            "resonance",
            "slope",
            "model",
            "drive",
            "key_track",
            "character",
        ] {
            out.push(format!("patch/filter[{index}]/{field}"));
        }
    }

    for index in 0..patch.envelopes.len() {
        for field in [
            "delay",
            "attack",
            "hold",
            "decay",
            "sustain",
            "release",
            "attack_shape",
            "decay_shape",
            "release_shape",
            "loop",
        ] {
            out.push(format!("patch/env[{index}]/{field}"));
        }
    }

    for index in 0..patch.lfos.len() {
        for field in [
            "wave",
            "rate",
            "sync",
            "division",
            "depth",
            "delay",
            "fade",
            "phase",
            "mode",
            "smooth",
            "draw",
            "grid",
            "shape_mode",
        ] {
            out.push(format!("patch/lfo[{index}]/{field}"));
        }
    }

    for index in 0..MACRO_COUNT {
        out.push(format!("patch/macro[{index}]"));
    }

    // A route's **depth** is a parameter; its source, destination, curve and
    // via are structure. That is the line §2.3 draws between an edit that goes
    // on the live wire and one that rebuilds the graph, and it is drawn here
    // by which of them has an address at all.
    for index in 0..patch.mod_matrix.routes.len() {
        out.push(format!("patch/mod[{index}]/depth"));
    }

    for (index, slot) in patch.fx.iter().enumerate() {
        out.push(format!("patch/fx[{index}]/enabled"));
        for spec in slot.config.specs() {
            out.push(format!("patch/fx[{index}]/{}", spec.id));
        }
    }

    out
}

/// Every modulation destination this patch actually has, with the label the
/// matrix row and the drag-to-assign ring show.
///
/// Built from the patch rather than from a fixed list, so a route to a layer
/// that is not there is not offerable — and so that the noise layer, which has
/// no position or warp, does not advertise them.
pub fn destinations(patch: &Patch) -> Vec<(ModDest, String)> {
    let mut out = Vec::new();
    for (index, layer) in patch.layers.iter().enumerate() {
        let Ok(i) = u8::try_from(index) else { continue };
        let role = layer_role(index).label();
        out.push((ModDest::LayerPitch(i), format!("{role} pitch")));
        out.push((ModDest::LayerGain(i), format!("{role} level")));
        out.push((ModDest::LayerPan(i), format!("{role} pan")));
        if let Source::Synth(osc) = &layer.source
            && !matches!(osc.source, SynthSource::Noise)
        {
            // Named for what the knob *is* on this kind of source, so a
            // route reads "OSC A bright" on a string rather than "position".
            let position = match osc.source {
                SynthSource::Sample(_) => "start",
                SynthSource::String => "bright",
                _ => "position",
            };
            out.push((ModDest::OscPosition(i), format!("{role} {position}")));
            out.push((ModDest::OscWarp(i), format!("{role} warp")));
            out.push((ModDest::OscUnisonDetune(i), format!("{role} detune")));
            out.push((ModDest::OscUnisonBlend(i), format!("{role} blend")));
        }
    }
    for index in 0..patch.filters.len() {
        let Ok(i) = u8::try_from(index) else { continue };
        let name = index + 1;
        out.push((ModDest::FilterCutoff(i), format!("Filter {name} cutoff")));
        out.push((ModDest::FilterResonance(i), format!("Filter {name} res")));
        out.push((ModDest::FilterDrive(i), format!("Filter {name} drive")));
        out.push((
            ModDest::FilterCharacter(i),
            format!("Filter {name} character"),
        ));
    }
    for index in 0..patch.envelopes.len() {
        let Ok(i) = u8::try_from(index) else { continue };
        let name = index + 1;
        // Stage 1 is attack, 3 decay, 4 sustain and 5 release, matching
        // `ModDest`'s own numbering of the AHDSR stages — every stage
        // `dest_address` maps, so a route the bank already makes (the Grand
        // Piano keys its release) has a name in the window rather than a
        // `Debug` print.
        out.push((
            ModDest::EnvelopeStageTime(i, 1),
            format!("Env {name} attack time"),
        ));
        out.push((
            ModDest::EnvelopeStageTime(i, 3),
            format!("Env {name} decay time"),
        ));
        out.push((
            ModDest::EnvelopeStageLevel(i, 4),
            format!("Env {name} sustain"),
        ));
        out.push((
            ModDest::EnvelopeStageTime(i, 5),
            format!("Env {name} release time"),
        ));
    }
    for index in 0..patch.lfos.len() {
        let Ok(i) = u8::try_from(index) else { continue };
        let name = index + 1;
        out.push((ModDest::LfoRate(i), format!("LFO {name} rate")));
        out.push((ModDest::LfoDepth(i), format!("LFO {name} depth")));
        out.push((ModDest::LfoPhase(i), format!("LFO {name} phase")));
    }
    out.push((ModDest::Amp, "Amp".to_string()));
    out
}

/// Which control on the panel a destination moves, as its §4 address.
///
/// **The join between a knob and the thing that modulates it.** Drag-to-assign
/// lands on a knob and has to write a `ModDest` into the matrix; the ring
/// round a knob has to know whether anything reaches it. Both questions are
/// this one, asked in the two directions, and `tests/flopsynth.rs` holds the
/// property that keeps it honest: every address here is one
/// [`addresses`] actually draws.
///
/// `None` for a destination with no knob of its own — [`ModDest::Amp`] is the
/// voice's own level, which the amp envelope already owns — and for the
/// destinations that belong to a *sampled* layer rather than a synth one.
pub fn dest_address(dest: ModDest) -> Option<String> {
    Some(match dest {
        ModDest::LayerPitch(i) => format!("patch/layer[{i}]/synth/semitones"),
        ModDest::LayerGain(i) => format!("patch/layer[{i}]/gain"),
        ModDest::LayerPan(i) => format!("patch/layer[{i}]/pan"),
        ModDest::OscPosition(i) => format!("patch/layer[{i}]/synth/position"),
        ModDest::OscWarp(i) => format!("patch/layer[{i}]/synth/warp"),
        ModDest::OscUnisonDetune(i) => format!("patch/layer[{i}]/synth/unison/detune"),
        ModDest::OscUnisonBlend(i) => format!("patch/layer[{i}]/synth/unison/blend"),
        ModDest::FilterCutoff(i) => format!("patch/filter[{i}]/cutoff"),
        ModDest::FilterResonance(i) => format!("patch/filter[{i}]/resonance"),
        ModDest::FilterDrive(i) => format!("patch/filter[{i}]/drive"),
        ModDest::FilterCharacter(i) => format!("patch/filter[{i}]/character"),
        // Stage 3 is decay and 4 is sustain — `ModDest`'s own numbering of the
        // AHDSR stages, which `destinations` follows too.
        ModDest::EnvelopeStageTime(i, 3) => format!("patch/env[{i}]/decay"),
        ModDest::EnvelopeStageTime(i, 1) => format!("patch/env[{i}]/attack"),
        ModDest::EnvelopeStageTime(i, 5) => format!("patch/env[{i}]/release"),
        ModDest::EnvelopeStageLevel(i, 4) => format!("patch/env[{i}]/sustain"),
        ModDest::LfoRate(i) => format!("patch/lfo[{i}]/rate"),
        ModDest::LfoDepth(i) => format!("patch/lfo[{i}]/depth"),
        ModDest::LfoPhase(i) => format!("patch/lfo[{i}]/phase"),
        // The rest belong to a sampler's layer or to the voice as a whole, and
        // have no knob on this panel.
        _ => return None,
    })
}

/// Which destination moves the control at `address`, if any.
///
/// The inverse of [`dest_address`], asked of the destinations *this patch
/// has* — so a route to a layer that is not there cannot be made, which is the
/// same rule [`destinations`] follows.
pub fn dest_for_address(patch: &Patch, address: &str) -> Option<ModDest> {
    destinations(patch)
        .into_iter()
        .find(|(dest, _)| dest_address(*dest).as_deref() == Some(address))
        .map(|(dest, _)| dest)
}

/// Every modulation source, with its label, in the order the Modulation page's
/// badge row shows them.
pub fn sources(patch: &Patch) -> Vec<(ModSource, String)> {
    let mut out = Vec::new();
    for index in 0..patch.envelopes.len().min(crate::MAX_MOD_ENVELOPES + 1) {
        let Ok(i) = u8::try_from(index) else { continue };
        out.push((ModSource::Envelope(i), format!("ENV {}", index + 1)));
    }
    for index in 0..patch.lfos.len().min(crate::MAX_LFOS) {
        let Ok(i) = u8::try_from(index) else { continue };
        out.push((ModSource::Lfo(i), format!("LFO {}", index + 1)));
    }
    for index in 0..MACRO_COUNT {
        let name = patch.macros[index].name.clone();
        let label = if name.is_empty() {
            format!("M{}", index + 1)
        } else {
            name
        };
        out.push((ModSource::Macro(index as u8), label));
    }
    out.extend([
        (ModSource::Velocity, "Velocity".to_string()),
        (ModSource::Key, "Key".to_string()),
        (ModSource::Aftertouch, "Aftertouch".to_string()),
        (ModSource::ModWheel, "Wheel".to_string()),
        (ModSource::PitchBend, "Bend".to_string()),
        (ModSource::Random, "Random".to_string()),
        (ModSource::NoteOnCounter, "Counter".to_string()),
        (ModSource::NoteModX, "Note X".to_string()),
        (ModSource::NoteModY, "Note Y".to_string()),
    ]);
    out
}

/// A sentence per thing this patch does with a performance control, for the
/// Presets page's About column.
///
/// Generated from the patch rather than stored beside it, so it is never
/// stale: a preset whose wheel route somebody removed stops claiming to have
/// one, with nothing to keep in step.
pub fn describe_routes(patch: &Patch) -> Vec<String> {
    let destinations = destinations(patch);
    let name_of = |dest: ModDest| {
        destinations
            .iter()
            .find(|(d, _)| *d == dest)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| "something".to_string())
    };
    let mut out = Vec::new();
    for (source, label) in [
        (ModSource::ModWheel, "Wheel"),
        (ModSource::Velocity, "Velocity"),
        (ModSource::Aftertouch, "Aftertouch"),
    ] {
        let targets: Vec<String> = patch
            .mod_matrix
            .routes
            .iter()
            .filter(|route| route.source == source || route.via == Some(source))
            .map(|route| name_of(route.destination))
            .collect();
        if !targets.is_empty() {
            out.push(format!("{label}: {}", targets.join(", ")));
        }
    }
    let named: Vec<&str> = patch
        .macros
        .iter()
        .filter(|m| !m.name.is_empty())
        .map(|m| m.name.as_str())
        .collect();
    if !named.is_empty() {
        out.push(format!("Macros: {}", named.join(", ")));
    }
    out
}
