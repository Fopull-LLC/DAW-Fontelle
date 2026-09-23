//! What the two rebuilt effects promise the document (`docs/effects-catalogue.md`
//! §3): a distortion that is a family of curves rather than five points, and a
//! bitcrusher that is a family of quantisers and decimators rather than one.
//!
//! The DSP claims are measured in `fontelle-fx/tests`. This file is the
//! **contract** side: every knob the catalogue names is a `ParamSpec` with the
//! right id, unit and positions; the sections a panel draws cover the table
//! exactly; a preset is a constructor that lands somewhere other than the
//! wire; and a project saved by the build before this one still opens with
//! the sound it had.

use fontelle_types::{
    BitcrushConfig, BitcrushPreset, CHORUS_TONE_OPEN_HZ, ChorusConfig, ChorusMode, Decimation,
    DistortionConfig, DistortionCurve, DistortionPreset, Dither, EffectConfig, EffectKind,
    FILTER_MOD_OCTAVES, FilterConfig, FilterShape, GATE_FLOOR_DB, GATE_KEY_OFF_HZ, GateConfig,
    LfoWave, MAX_CHORUS_DELAY_MS, MAX_CHORUS_VOICES, MAX_FILTER_HZ, MAX_GATE_LOOKAHEAD_MS,
    MAX_GATE_RATIO, MAX_LFO_RATE_HZ, MAX_TUNE_GRAIN_MS, MAX_TUNE_RETUNE_MS, MIN_CHORUS_DELAY_MS,
    MIN_FILTER_HZ, MIN_LFO_RATE_HZ, MIN_TUNE_GRAIN_MS, MIN_TUNE_RETUNE_MS, NoteDivision,
    Oversampling, Quantiser, SoftenConfig, SoftenPreset, TUNE_NOTE_PARAMS, Taper, TuneConfig,
    TuneControl, TuneEngine, TuneMode, TunePreset, TuneRange, TuneScale, UTILITY_DC_OFF_HZ,
    UTILITY_MONO_OFF_HZ, Unit, UtilityConfig, VibratoShape,
};

fn spec_of(config: &EffectConfig, id: &str) -> fontelle_types::ParamSpec {
    *config
        .specs()
        .iter()
        .find(|spec| spec.id == id)
        .unwrap_or_else(|| panic!("{:?} has no parameter {id}", config.kind()))
}

// ------------------------------------------------------------- sections

/// Every effect declares sections, and they cover its parameter table
/// exactly — no control left out of the panel, none drawn twice.
#[test]
fn every_effects_sections_cover_its_parameters_exactly() {
    for kind in EffectKind::ALL {
        let config = EffectConfig::new(kind);
        let sections = config.sections();
        assert!(!sections.is_empty(), "{kind:?} declares no sections");
        let covered: usize = sections.iter().map(|section| section.count).sum();
        assert_eq!(
            covered,
            config.specs().len(),
            "{kind:?}'s sections cover {covered} of {} parameters",
            config.specs().len()
        );
        for section in sections {
            assert!(!section.name.is_empty(), "{kind:?} has a nameless section");
            assert!(section.count > 0, "{kind:?} has an empty section");
        }
    }
}

#[test]
fn the_distortion_is_drive_then_voicing_then_output() {
    let config = EffectConfig::new(EffectKind::Distortion);
    let names: Vec<&str> = config.sections().iter().map(|s| s.name).collect();
    assert_eq!(names, ["Drive", "Voicing", "Output"]);
    // And the ids fall in those sections, in the catalogue's order.
    let ids: Vec<&str> = config.specs().iter().map(|spec| spec.id).collect();
    assert_eq!(
        ids,
        [
            "curve",
            "shape",
            "drive",
            "bias",
            "sag", //
            "pre_hp",
            "pre_mid_hz",
            "pre_mid_db",
            "clean_low",
            "tone", //
            "output",
            "auto_gain",
            "oversample",
            "mix",
        ]
    );
}

#[test]
fn the_bitcrush_is_depth_then_rate_then_output() {
    let config = EffectConfig::new(EffectKind::Bitcrush);
    let names: Vec<&str> = config.sections().iter().map(|s| s.name).collect();
    assert_eq!(names, ["Depth", "Rate", "Output"]);
    let ids: Vec<&str> = config.specs().iter().map(|spec| spec.id).collect();
    assert_eq!(
        ids,
        [
            "input",
            "bits",
            "quantiser",
            "dither", //
            "rate",
            "decimation",
            "jitter",
            "antialias", //
            "post_lp",
            "output",
            "mix",
        ]
    );
}

// ------------------------------------------------------------ distortion

#[test]
fn the_distortion_offers_ten_curves_and_the_chooser_names_them_all() {
    assert_eq!(DistortionCurve::ALL.len(), 10);
    let config = EffectConfig::new(EffectKind::Distortion);
    let curve = spec_of(&config, "curve");
    assert_eq!(curve.taper, Taper::Stepped(10));
    let labels: Vec<&str> = DistortionCurve::ALL.iter().map(|c| c.label()).collect();
    assert_eq!(curve.positions, labels.as_slice());
    // The five that existed keep their names, so a saved project's curve
    // still reads as the curve it was.
    for label in ["soft clip", "hard clip", "tube", "fold", "wave shape"] {
        assert!(labels.contains(&label), "{label} went missing");
    }
    for label in ["diode", "triangle fold", "rectify", "crossover", "wrap"] {
        assert!(labels.contains(&label), "{label} is not offered");
    }
}

#[test]
fn every_curve_says_what_its_shape_knob_does() {
    // A `shape` that means nothing on a curve is a dead knob on that
    // position of the chooser. The word is what the tooltip and the docs say.
    for curve in DistortionCurve::ALL {
        assert!(
            !curve.shape_meaning().is_empty(),
            "{curve:?} does not say what shape does"
        );
    }
}

#[test]
fn the_distortions_new_knobs_have_the_units_and_ranges_the_catalogue_gives() {
    let config = EffectConfig::new(EffectKind::Distortion);
    let shape = spec_of(&config, "shape");
    assert_eq!(
        (shape.unit, shape.min, shape.max),
        (Unit::Percent, 0.0, 100.0)
    );
    let bias = spec_of(&config, "bias");
    assert_eq!(
        (bias.unit, bias.min, bias.max, bias.default),
        (Unit::Percent, -100.0, 100.0, 0.0)
    );
    let sag = spec_of(&config, "sag");
    assert_eq!((sag.unit, sag.default), (Unit::Percent, 0.0));
    let pre_hp = spec_of(&config, "pre_hp");
    assert_eq!(
        (pre_hp.unit, pre_hp.min, pre_hp.max),
        (Unit::Hertz, 20.0, 2_000.0)
    );
    assert_eq!(pre_hp.taper, Taper::Logarithmic);
    let mid_hz = spec_of(&config, "pre_mid_hz");
    assert_eq!(
        (mid_hz.unit, mid_hz.min, mid_hz.max),
        (Unit::Hertz, 200.0, 5_000.0)
    );
    let mid_db = spec_of(&config, "pre_mid_db");
    assert_eq!(
        (mid_db.unit, mid_db.min, mid_db.max, mid_db.default),
        (Unit::Decibels, -18.0, 18.0, 0.0)
    );
    let clean = spec_of(&config, "clean_low");
    assert_eq!(
        (clean.unit, clean.min, clean.max, clean.default),
        (Unit::Hertz, 20.0, 500.0, 20.0)
    );
    let auto = spec_of(&config, "auto_gain");
    assert_eq!(auto.unit, Unit::Switch);
    let os = spec_of(&config, "oversample");
    assert_eq!(os.taper, Taper::Stepped(4));
    assert_eq!(os.positions, ["off", "2x", "4x", "8x"]);
}

#[test]
fn oversampling_is_a_factor() {
    assert_eq!(Oversampling::Off.factor(), 1);
    assert_eq!(Oversampling::Two.factor(), 2);
    assert_eq!(Oversampling::Four.factor(), 4);
    assert_eq!(Oversampling::Eight.factor(), 8);
    let labels: Vec<&str> = Oversampling::ALL.iter().map(|o| o.label()).collect();
    assert_eq!(labels, ["off", "2x", "4x", "8x"]);
}

#[test]
fn a_fresh_distortion_is_a_wire_with_every_new_stage_at_rest() {
    // Rule 2 of the catalogue: the new stages must not commit anybody to a
    // tone. A fresh distortion opens exactly as the old one did.
    let config = DistortionConfig::new();
    assert_eq!(config.curve, DistortionCurve::SoftClip);
    assert_eq!(config.drive_db, 0.0);
    assert_eq!(config.bias, 0.0);
    assert_eq!(config.sag, 0.0);
    assert_eq!(config.pre_hp_hz, 20.0);
    assert_eq!(config.pre_mid_db, 0.0);
    assert_eq!(config.clean_low_hz, 20.0);
    assert_eq!(config.tone_hz, 20_000.0);
    assert_eq!(config.output_db, 0.0);
    assert_eq!(config.oversample, Oversampling::Two);
    assert_eq!(config.mix, 1.0);
}

#[test]
fn every_distortion_preset_is_somewhere_other_than_the_wire_and_than_each_other() {
    let wire = DistortionConfig::new();
    let mut seen: Vec<DistortionConfig> = Vec::new();
    for preset in DistortionPreset::ALL {
        let config = DistortionConfig::from_preset(preset);
        assert_ne!(config, wire, "{preset:?} is the wire");
        assert!(
            config.drive_db > 0.0,
            "{preset:?} has no drive, which is not a distortion preset"
        );
        assert!(!preset.label().is_empty());
        for other in &seen {
            assert_ne!(config, *other, "{preset:?} duplicates another preset");
        }
        seen.push(config);
    }
    assert_eq!(DistortionPreset::ALL.len(), 7);
}

#[test]
fn the_distortions_choosers_are_reachable_through_their_addresses() {
    let mut config = EffectConfig::new(EffectKind::Distortion);
    config.set("curve", 9.0);
    config.set("oversample", 3.0);
    config.set("bias", -40.0);
    config.set("auto_gain", 0.0);
    let EffectConfig::Distortion(dist) = config else {
        unreachable!()
    };
    assert_eq!(dist.curve, DistortionCurve::Wrap);
    assert_eq!(dist.oversample, Oversampling::Eight);
    assert!((dist.bias + 0.4).abs() < 1e-6);
    assert!(!dist.auto_gain);
}

/// A project saved before this pass carried `"oversample": true`. It still
/// opens, and it opens at the two-times the switch meant.
#[test]
fn a_distortion_saved_with_the_old_switch_reads_as_two_times() {
    let old = r#"{
        "curve": "HardClip",
        "drive_db": 12.0,
        "tone_hz": 5000.0,
        "output_db": -3.0,
        "oversample": true,
        "mix": 0.8
    }"#;
    let config: DistortionConfig = serde_json::from_str(old).expect("the old shape loads");
    assert_eq!(config.curve, DistortionCurve::HardClip);
    assert_eq!(config.oversample, Oversampling::Two);
    assert_eq!(config.drive_db, 12.0);
    // And everything the old file could not say is at rest.
    assert_eq!(config.bias, 0.0);
    assert_eq!(config.sag, 0.0);
    assert_eq!(config.pre_hp_hz, 20.0);
    assert_eq!(config.clean_low_hz, 20.0);

    let off = old.replace("true", "false");
    let config: DistortionConfig = serde_json::from_str(&off).unwrap();
    assert_eq!(config.oversample, Oversampling::Off);
}

#[test]
fn a_distortion_round_trips_through_json_with_its_new_fields() {
    let config = DistortionConfig::from_preset(DistortionPreset::Fuzz);
    let text = serde_json::to_string(&config).unwrap();
    let back: DistortionConfig = serde_json::from_str(&text).unwrap();
    assert_eq!(back, config);
}

// -------------------------------------------------------------- bitcrush

#[test]
fn the_bitcrush_offers_three_quantisers_four_dithers_and_three_decimators() {
    let config = EffectConfig::new(EffectKind::Bitcrush);
    let quantiser = spec_of(&config, "quantiser");
    assert_eq!(quantiser.taper, Taper::Stepped(3));
    let labels: Vec<&str> = Quantiser::ALL.iter().map(|q| q.label()).collect();
    assert_eq!(quantiser.positions, labels.as_slice());
    assert_eq!(labels, ["round", "truncate", "mu-law"]);

    let dither = spec_of(&config, "dither");
    assert_eq!(dither.taper, Taper::Stepped(4));
    let labels: Vec<&str> = Dither::ALL.iter().map(|d| d.label()).collect();
    assert_eq!(dither.positions, labels.as_slice());
    assert_eq!(labels, ["off", "rectangular", "triangular", "shaped"]);

    let decimation = spec_of(&config, "decimation");
    assert_eq!(decimation.taper, Taper::Stepped(3));
    let labels: Vec<&str> = Decimation::ALL.iter().map(|d| d.label()).collect();
    assert_eq!(decimation.positions, labels.as_slice());
    assert_eq!(labels, ["hold", "linear", "drop"]);
}

#[test]
fn the_bitcrushs_new_knobs_have_the_units_and_ranges_the_catalogue_gives() {
    let config = EffectConfig::new(EffectKind::Bitcrush);
    let input = spec_of(&config, "input");
    assert_eq!(
        (input.unit, input.min, input.max, input.default),
        (Unit::Decibels, -24.0, 24.0, 0.0)
    );
    let output = spec_of(&config, "output");
    assert_eq!(
        (output.unit, output.min, output.max, output.default),
        (Unit::Decibels, -24.0, 24.0, 0.0)
    );
    let jitter = spec_of(&config, "jitter");
    assert_eq!(
        (jitter.unit, jitter.min, jitter.max, jitter.default),
        (Unit::Percent, 0.0, 100.0, 0.0)
    );
    let post = spec_of(&config, "post_lp");
    assert_eq!(
        (post.unit, post.min, post.max, post.default),
        (Unit::Hertz, 200.0, 20_000.0, 20_000.0)
    );
    assert_eq!(post.taper, Taper::Logarithmic);
}

#[test]
fn a_fresh_bitcrush_is_a_wire_with_every_new_stage_at_rest() {
    let config = BitcrushConfig::new();
    assert_eq!(config.input_db, 0.0);
    assert_eq!(config.bits, 16.0);
    assert_eq!(config.quantiser, Quantiser::Round);
    assert_eq!(config.dither, Dither::Off);
    assert_eq!(config.decimation, Decimation::Hold);
    assert_eq!(config.jitter, 0.0);
    assert!(!config.anti_alias);
    assert_eq!(config.post_lp_hz, 20_000.0);
    assert_eq!(config.output_db, 0.0);
}

#[test]
fn every_bitcrush_preset_is_somewhere_other_than_the_wire_and_than_each_other() {
    let wire = BitcrushConfig::new();
    let mut seen: Vec<BitcrushConfig> = Vec::new();
    for preset in BitcrushPreset::ALL {
        let config = BitcrushConfig::from_preset(preset);
        assert_ne!(config, wire, "{preset:?} is the wire");
        assert!(!preset.label().is_empty());
        for other in &seen {
            assert_ne!(config, *other, "{preset:?} duplicates another preset");
        }
        seen.push(config);
    }
    assert_eq!(BitcrushPreset::ALL.len(), 6);
}

#[test]
fn the_bitcrushs_choosers_are_reachable_through_their_addresses() {
    let mut config = EffectConfig::new(EffectKind::Bitcrush);
    config.set("quantiser", 2.0);
    config.set("dither", 3.0);
    config.set("decimation", 1.0);
    config.set("jitter", 25.0);
    let EffectConfig::Bitcrush(crush) = config else {
        unreachable!()
    };
    assert_eq!(crush.quantiser, Quantiser::MuLaw);
    assert_eq!(crush.dither, Dither::Shaped);
    assert_eq!(crush.decimation, Decimation::Linear);
    assert!((crush.jitter - 0.25).abs() < 1e-6);
}

/// A project saved before this pass carried `"dither": true`. It still
/// opens, and it opens at the triangular dither the switch meant.
#[test]
fn a_bitcrush_saved_with_the_old_switch_reads_as_triangular_dither() {
    let old = r#"{
        "bits": 8.0,
        "rate_hz": 11025.0,
        "dither": true,
        "anti_alias": false,
        "mix": 1.0
    }"#;
    let config: BitcrushConfig = serde_json::from_str(old).expect("the old shape loads");
    assert_eq!(config.dither, Dither::Triangular);
    assert_eq!(config.bits, 8.0);
    assert_eq!(config.rate_hz, 11_025.0);
    assert_eq!(config.quantiser, Quantiser::Round);
    assert_eq!(config.decimation, Decimation::Hold);
    assert_eq!(config.input_db, 0.0);

    let off = old.replace("true", "false");
    let config: BitcrushConfig = serde_json::from_str(&off).unwrap();
    assert_eq!(config.dither, Dither::Off);
}

#[test]
fn a_bitcrush_round_trips_through_json_with_its_new_fields() {
    let config = BitcrushConfig::from_preset(BitcrushPreset::Telephone);
    let text = serde_json::to_string(&config).unwrap();
    let back: BitcrushConfig = serde_json::from_str(&text).unwrap();
    assert_eq!(back, config);
}

// --------------------------------------------------------------- utility

#[test]
fn the_utility_is_level_then_stereo_then_channels_then_output() {
    let config = EffectConfig::new(EffectKind::Utility);
    let names: Vec<&str> = config.sections().iter().map(|s| s.name).collect();
    assert_eq!(names, ["Level", "Stereo", "Channels", "Output"]);
    // The order a repair happens in, which is the order the panel reads in.
    let ids: Vec<&str> = config.specs().iter().map(|spec| spec.id).collect();
    assert_eq!(
        ids,
        [
            "gain", "pan", //
            "width", "mono", "swap", //
            "mute_l", "mute_r", "invert_l", "invert_r", //
            "dc", "mix",
        ]
    );
}

#[test]
fn the_utility_is_one_insert_and_not_seven() {
    // The catalogue's §2.5 claim, as a count: gain, pan, width, the
    // mono-maker, the swap, two mutes, two polarity flips and the rumble
    // filter, in one place. Seven entries in the "+ Add effect" menu would
    // make the commonest thing in mixing the fiddliest.
    assert_eq!(EffectConfig::new(EffectKind::Utility).specs().len(), 11);
    assert!(EffectKind::ALL.contains(&EffectKind::Utility));
}

#[test]
fn the_utilitys_knobs_have_the_units_and_ranges_the_catalogue_gives() {
    let config = EffectConfig::new(EffectKind::Utility);

    let gain = spec_of(&config, "gain");
    assert_eq!(
        (gain.unit, gain.min, gain.max, gain.default),
        (Unit::Decibels, -24.0, 24.0, 0.0)
    );

    let pan = spec_of(&config, "pan");
    assert_eq!(
        (pan.unit, pan.min, pan.max, pan.default),
        (Unit::Percent, -100.0, 100.0, 0.0)
    );

    let width = spec_of(&config, "width");
    assert_eq!(
        (width.unit, width.min, width.max, width.default),
        (Unit::Percent, 0.0, 200.0, 100.0)
    );

    // Both filters are frequencies, so both are aimed logarithmically: a
    // linear 20 Hz–500 Hz lane spends most of its travel where no mix
    // decision lives.
    for id in ["mono", "dc"] {
        let spec = spec_of(&config, id);
        assert_eq!(spec.unit, Unit::Hertz, "{id} is not a frequency");
        assert_eq!(spec.taper, Taper::Logarithmic, "{id} is aimed linearly");
        assert_eq!(spec.max, 500.0, "{id} does not reach the rumble region");
    }
    assert_eq!(spec_of(&config, "mono").min, UTILITY_MONO_OFF_HZ);
    assert_eq!(spec_of(&config, "dc").min, UTILITY_DC_OFF_HZ);
}

#[test]
fn every_one_of_the_utilitys_switches_is_a_switch() {
    // A lane that drew a ramp through a polarity flip would be a lane nobody
    // could aim — see `Unit::Switch`.
    let config = EffectConfig::new(EffectKind::Utility);
    for id in ["swap", "mute_l", "mute_r", "invert_l", "invert_r"] {
        let spec = spec_of(&config, id);
        assert_eq!(spec.unit, Unit::Switch, "{id} is not drawn as a switch");
        assert_eq!(
            spec.taper,
            Taper::Stepped(2),
            "{id} has more than two states"
        );
        assert_eq!(
            spec.positions,
            ["off", "on"],
            "{id} does not name its states"
        );
        assert_eq!(spec.default, 0.0, "{id} is on before anybody asked");
    }
}

#[test]
fn a_fresh_utility_is_a_wire_in_every_one_of_its_controls() {
    let utility = UtilityConfig::new();
    assert_eq!(utility.gain_db, 0.0);
    assert_eq!(utility.pan, 0.0);
    assert_eq!(utility.width, 1.0, "a fresh utility is not 100 % wide");
    assert_eq!(utility.mono_below_hz, UTILITY_MONO_OFF_HZ);
    assert_eq!(utility.dc_hz, UTILITY_DC_OFF_HZ);
    assert!(!utility.swap);
    assert!(!utility.mute_left && !utility.mute_right);
    assert!(!utility.invert_left && !utility.invert_right);
    assert_eq!(utility.mix, 1.0);
}

#[test]
fn the_utilitys_switches_are_reachable_through_their_addresses() {
    // What automation and MIDI learn write, so a flip is something a lane can
    // own rather than something only the mouse can reach (INVARIANT 7).
    let mut config = EffectConfig::new(EffectKind::Utility);
    config.set("invert_r", 1.0);
    config.set("mute_l", 1.0);
    config.set("swap", 1.0);
    config.set("width", 200.0);
    config.set("pan", -100.0);
    let EffectConfig::Utility(utility) = config else {
        unreachable!()
    };
    assert!(utility.invert_right && utility.mute_left && utility.swap);
    assert!(!utility.invert_left, "one switch moved another");
    assert_eq!(utility.width, 2.0);
    assert_eq!(utility.pan, -1.0);
}

#[test]
fn a_utility_round_trips_through_json() {
    let config = UtilityConfig {
        gain_db: -3.5,
        pan: 0.25,
        width: 1.4,
        mono_below_hz: 120.0,
        swap: true,
        mute_left: false,
        mute_right: true,
        invert_left: true,
        invert_right: false,
        dc_hz: 40.0,
        mix: 0.8,
    };
    let text = serde_json::to_string(&config).unwrap();
    let back: UtilityConfig = serde_json::from_str(&text).unwrap();
    assert_eq!(back, config);
}

// ------------------------------------------------------------------ gate

#[test]
fn the_gate_is_detection_then_envelope_then_amount() {
    let config = EffectConfig::new(EffectKind::Gate);
    let names: Vec<&str> = config.sections().iter().map(|s| s.name).collect();
    assert_eq!(names, ["Detection", "Envelope", "Amount"]);
    // What it listens to, how it moves, how far down it goes.
    let ids: Vec<&str> = config.specs().iter().map(|spec| spec.id).collect();
    assert_eq!(
        ids,
        [
            "threshold",
            "hysteresis",
            "key_hp",
            "lookahead", //
            "attack",
            "hold",
            "release", //
            "ratio",
            "range",
            "mix",
        ]
    );
}

#[test]
fn the_gate_and_the_expander_are_one_effect() {
    // The catalogue's §2.1 claim: a gate is an expander at a steep ratio and
    // a deep range, and both of those are knobs, so everything between the
    // two — the expander that ducks spill by six decibels — is reachable
    // rather than being a third entry in the menu.
    let config = EffectConfig::new(EffectKind::Gate);
    let ratio = spec_of(&config, "ratio");
    assert_eq!(ratio.min, 1.0, "a gate cannot be turned off");
    assert_eq!(ratio.max, MAX_GATE_RATIO);
    assert_eq!(ratio.taper, Taper::Logarithmic);
    let range = spec_of(&config, "range");
    assert_eq!(
        (range.unit, range.min, range.max),
        (Unit::Decibels, GATE_FLOOR_DB, 0.0),
        "the range does not reach from a gate to an expander"
    );
}

#[test]
fn the_gates_knobs_have_the_units_and_ranges_the_catalogue_gives() {
    let config = EffectConfig::new(EffectKind::Gate);

    let threshold = spec_of(&config, "threshold");
    assert_eq!(
        (
            threshold.unit,
            threshold.min,
            threshold.max,
            threshold.default
        ),
        (Unit::Decibels, GATE_FLOOR_DB, 0.0, GATE_FLOOR_DB)
    );

    let hysteresis = spec_of(&config, "hysteresis");
    assert_eq!(
        (hysteresis.unit, hysteresis.min, hysteresis.max),
        (Unit::Decibels, 0.0, 24.0)
    );
    assert!(
        hysteresis.default > 0.0,
        "a gate with no hysteresis chatters, and that is not a default"
    );

    let key = spec_of(&config, "key_hp");
    assert_eq!(
        (key.unit, key.taper, key.min, key.default),
        (
            Unit::Hertz,
            Taper::Logarithmic,
            GATE_KEY_OFF_HZ,
            GATE_KEY_OFF_HZ
        )
    );

    // The three times. Each one is milliseconds, which is what the
    // compressor's attack read as "10 s" before a unit could say otherwise.
    for id in ["lookahead", "attack", "hold", "release"] {
        assert_eq!(
            spec_of(&config, id).unit,
            Unit::Milliseconds,
            "{id} is not in milliseconds"
        );
    }
    let lookahead = spec_of(&config, "lookahead");
    assert_eq!(
        (lookahead.min, lookahead.max, lookahead.default),
        (0.0, MAX_GATE_LOOKAHEAD_MS, 0.0)
    );
    // Both of the times that have to be able to read exactly zero are linear,
    // because a logarithmic taper has no bottom.
    for id in ["lookahead", "hold"] {
        let spec = spec_of(&config, id);
        assert_eq!(spec.taper, Taper::Linear, "{id} cannot reach zero");
        assert_eq!(spec.min, 0.0);
    }
}

#[test]
fn a_fresh_gate_is_a_gate_with_its_threshold_at_never() {
    // A different reading of "a fresh effect is nearly a wire" from the
    // compressor's 1:1, and the reason is written on `GateConfig::new`: the
    // knob a person reaches for on a gate is the threshold, so that is the
    // one that opens at "off" and the rest open at gate settings.
    let gate = GateConfig::new();
    assert_eq!(gate.threshold_db, GATE_FLOOR_DB);
    assert_eq!(gate.ratio, MAX_GATE_RATIO, "a fresh gate is not a gate");
    assert_eq!(gate.range_db, GATE_FLOOR_DB);
    assert_eq!(gate.lookahead_ms, 0.0, "a fresh insert costs latency");
    assert_eq!(gate.key_hp_hz, GATE_KEY_OFF_HZ);
    assert!(gate.hysteresis_db > 0.0);
    assert!(gate.hold_ms > 0.0, "a fresh gate cuts a decay in half");
    assert_eq!(gate.mix, 1.0);
}

#[test]
fn the_gates_knobs_are_reachable_through_their_addresses() {
    let mut config = EffectConfig::new(EffectKind::Gate);
    for (id, value) in [
        ("threshold", -24.0f32),
        ("hysteresis", 6.0),
        ("key_hp", 400.0),
        ("lookahead", 4.0),
        ("attack", 2.0),
        ("hold", 120.0),
        ("release", 250.0),
        ("ratio", 4.0),
        ("range", -12.0),
    ] {
        config.set(id, value);
        let read = config
            .get(id)
            .unwrap_or_else(|| panic!("{id} is not readable"));
        assert!((read - value).abs() < 1e-3, "{id} read back as {read}");
    }
    let EffectConfig::Gate(gate) = config else {
        unreachable!()
    };
    assert_eq!(gate.threshold_db, -24.0);
    assert_eq!(gate.range_db, -12.0);
}

#[test]
fn a_gate_round_trips_through_json() {
    let config = GateConfig {
        threshold_db: -32.0,
        hysteresis_db: 8.0,
        key_hp_hz: 300.0,
        lookahead_ms: 3.0,
        attack_ms: 0.5,
        hold_ms: 60.0,
        release_ms: 400.0,
        ratio: 8.0,
        range_db: -18.0,
        mix: 0.75,
    };
    let text = serde_json::to_string(&config).unwrap();
    let back: GateConfig = serde_json::from_str(&text).unwrap();
    assert_eq!(back, config);
}

// ---------------------------------------------------------------- chorus

#[test]
fn the_chorus_is_voices_then_modulation_then_output() {
    let config = EffectConfig::new(EffectKind::Chorus);
    let names: Vec<&str> = config.sections().iter().map(|s| s.name).collect();
    assert_eq!(names, ["Voices", "Modulation", "Output"]);
    let ids: Vec<&str> = config.specs().iter().map(|spec| spec.id).collect();
    assert_eq!(
        ids,
        [
            "voices", "mode", "spread", //
            "rate", "sync", "division", "depth", "delay", //
            "feedback", "tone", "mix",
        ]
    );
}

#[test]
fn the_chorus_offers_both_modes_and_the_chooser_names_them() {
    // Rule 1: a chooser names *kinds*. One LFO shared and one LFO each are
    // two machines, not two settings of one.
    assert_eq!(ChorusMode::ALL.len(), 2);
    let config = EffectConfig::new(EffectKind::Chorus);
    let mode = spec_of(&config, "mode");
    assert_eq!(mode.taper, Taper::Stepped(2));
    let labels: Vec<&str> = ChorusMode::ALL.iter().map(|m| m.label()).collect();
    assert_eq!(mode.positions, labels.as_slice());
}

#[test]
fn the_choruss_knobs_have_the_units_and_ranges_the_catalogue_gives() {
    let config = EffectConfig::new(EffectKind::Chorus);

    let voices = spec_of(&config, "voices");
    assert_eq!(
        (voices.min, voices.max, voices.taper),
        (
            1.0,
            MAX_CHORUS_VOICES as f32,
            Taper::Stepped(MAX_CHORUS_VOICES)
        )
    );
    assert!(
        voices.positions.is_empty(),
        "a count is a number, not a set of names"
    );

    let delay = spec_of(&config, "delay");
    assert_eq!(
        (delay.unit, delay.min, delay.max),
        (Unit::Milliseconds, MIN_CHORUS_DELAY_MS, MAX_CHORUS_DELAY_MS),
        "the centre does not cover the range a chorus lives in"
    );

    let rate = spec_of(&config, "rate");
    assert_eq!(
        (rate.unit, rate.taper, rate.min, rate.max),
        (
            Unit::Hertz,
            Taper::Logarithmic,
            MIN_LFO_RATE_HZ,
            MAX_LFO_RATE_HZ
        )
    );

    // Signed feedback, which is the whole point of that control: the same
    // comb with its teeth in the gaps is a different sound.
    let feedback = spec_of(&config, "feedback");
    assert_eq!(
        (feedback.unit, feedback.min, feedback.max, feedback.default),
        (Unit::Percent, -90.0, 90.0, 0.0)
    );

    let tone = spec_of(&config, "tone");
    assert_eq!(
        (tone.unit, tone.max, tone.default),
        (Unit::Hertz, CHORUS_TONE_OPEN_HZ, CHORUS_TONE_OPEN_HZ),
        "a fresh chorus is filtering"
    );
}

#[test]
fn the_chorus_follows_the_song_the_way_rule_five_says() {
    // Every time-based control has a sync switch and a note-value chooser
    // beside its own knob, and the chooser names the same divisions the
    // delay's does — one list, so a person who has learned one has learned
    // both.
    let config = EffectConfig::new(EffectKind::Chorus);
    let sync = spec_of(&config, "sync");
    assert_eq!(sync.unit, Unit::Switch);
    assert_eq!(sync.default, 0.0);
    let division = spec_of(&config, "division");
    let delay = EffectConfig::new(EffectKind::Delay);
    assert_eq!(
        division.positions,
        spec_of(&delay, "division").positions,
        "the chorus and the delay name their divisions differently"
    );
    assert_eq!(
        division.taper,
        Taper::Stepped(NoteDivision::ALL.len() as u32)
    );
}

#[test]
fn a_fresh_chorus_is_a_chorus_and_it_sits_under_the_track() {
    // The delay's reading of "fresh", not the compressor's: what this writes
    // is the voices with none of the signal that made them, so an instance at
    // rest would be a track replaced by silence rather than a wire.
    assert!(EffectKind::Chorus.is_time_based());
    let chorus = ChorusConfig::new();
    assert_eq!(chorus.voices, 2);
    assert_eq!(chorus.mode, ChorusMode::Chorus);
    assert!(chorus.depth > 0.0, "a fresh chorus does not move");
    assert!(chorus.spread > 0.0, "a fresh chorus is mono");
    assert_eq!(chorus.feedback, 0.0, "a fresh chorus is a flanger");
    assert_eq!(chorus.tone_hz, CHORUS_TONE_OPEN_HZ);
    assert!(
        chorus.mix > 0.0 && chorus.mix < 1.0,
        "a fully wet chorus is a vibrato: {}",
        chorus.mix
    );
}

#[test]
fn the_choruss_knobs_are_reachable_through_their_addresses() {
    let mut config = EffectConfig::new(EffectKind::Chorus);
    config.set("voices", 4.0);
    config.set("mode", 1.0);
    config.set("spread", 100.0);
    config.set("feedback", -60.0);
    config.set("delay", 8.0);
    config.set("sync", 1.0);
    let EffectConfig::Chorus(chorus) = config else {
        unreachable!()
    };
    assert_eq!(chorus.voices, MAX_CHORUS_VOICES);
    assert_eq!(chorus.mode, ChorusMode::Ensemble);
    assert_eq!(chorus.spread, 1.0);
    assert!((chorus.feedback + 0.6).abs() < 1e-6);
    assert_eq!(chorus.delay_ms, 8.0);
    assert!(chorus.sync);
}

#[test]
fn the_voice_count_cannot_be_driven_past_what_there_are() {
    // An automation lane reaching the top of its travel must produce four
    // voices, not five — and the bottom must produce one, not zero, which
    // would be an effect that silently stopped.
    let mut config = EffectConfig::new(EffectKind::Chorus);
    for asked in [-5.0f32, 0.0, 1.0, 4.0, 99.0] {
        config.set("voices", asked);
        let EffectConfig::Chorus(chorus) = config else {
            unreachable!()
        };
        assert!(
            (1..=MAX_CHORUS_VOICES).contains(&chorus.voices),
            "asking for {asked} voices gave {}",
            chorus.voices
        );
    }
}

#[test]
fn a_chorus_round_trips_through_json() {
    let config = ChorusConfig {
        voices: 3,
        mode: ChorusMode::Ensemble,
        spread: 0.8,
        rate_hz: 1.25,
        sync: true,
        division: NoteDivision::Half,
        depth: 0.7,
        delay_ms: 22.0,
        feedback: -0.4,
        tone_hz: 5_000.0,
        mix: 0.4,
    };
    let text = serde_json::to_string(&config).unwrap();
    let back: ChorusConfig = serde_json::from_str(&text).unwrap();
    assert_eq!(back, config);
}

// ---------------------------------------------------------------- filter

#[test]
fn the_filter_is_the_filter_then_the_two_things_that_move_it() {
    let config = EffectConfig::new(EffectKind::Filter);
    let names: Vec<&str> = config.sections().iter().map(|s| s.name).collect();
    assert_eq!(names, ["Filter", "Envelope", "LFO", "Output"]);
    let ids: Vec<&str> = config.specs().iter().map(|spec| spec.id).collect();
    assert_eq!(
        ids,
        [
            "shape",
            "cutoff",
            "resonance",
            "drive", //
            "env",
            "env_attack",
            "env_release", //
            "lfo",
            "lfo_rate",
            "lfo_sync",
            "lfo_division",
            "lfo_wave", //
            "output",
            "mix",
        ]
    );
}

#[test]
fn the_filter_offers_every_shape_the_catalogue_names() {
    // LP, HP and BP at both slopes, plus a notch and a peak — and the chooser
    // says which is which, because a filter type reading "3.00" is a control
    // nobody can set on purpose.
    assert_eq!(FilterShape::ALL.len(), 8);
    let config = EffectConfig::new(EffectKind::Filter);
    let shape = spec_of(&config, "shape");
    assert_eq!(shape.taper, Taper::Stepped(8));
    let labels: Vec<&str> = FilterShape::ALL.iter().map(|s| s.label()).collect();
    assert_eq!(shape.positions, labels.as_slice());
    // The number in a name is its slope, and the type says so.
    for shape in FilterShape::ALL {
        let expected = usize::from(shape.label().ends_with("24")) + 1;
        assert_eq!(
            shape.sections(),
            expected,
            "{shape:?} is built from the wrong number of sections"
        );
    }
}

#[test]
fn the_filters_knobs_have_the_units_and_ranges_the_catalogue_gives() {
    let config = EffectConfig::new(EffectKind::Filter);

    let cutoff = spec_of(&config, "cutoff");
    assert_eq!(
        (
            cutoff.unit,
            cutoff.taper,
            cutoff.min,
            cutoff.max,
            cutoff.default
        ),
        (
            Unit::Hertz,
            Taper::Logarithmic,
            MIN_FILTER_HZ,
            MAX_FILTER_HZ,
            MAX_FILTER_HZ
        ),
        "the cutoff does not span the band, or does not open at the top"
    );

    // The envelope's amount is signed: a filter that closes as the signal
    // gets loud is the half of an auto-wah nobody ships.
    let env = spec_of(&config, "env");
    assert_eq!(
        (env.unit, env.min, env.max, env.default),
        (Unit::Percent, -100.0, 100.0, 0.0)
    );
    // The LFO's is not — it is a depth either side of wherever the envelope
    // left the corner, and a negative depth is the same sweep.
    let lfo = spec_of(&config, "lfo");
    assert_eq!(
        (lfo.unit, lfo.min, lfo.max, lfo.default),
        (Unit::Percent, 0.0, 100.0, 0.0)
    );

    let rate = spec_of(&config, "lfo_rate");
    assert_eq!(
        (rate.unit, rate.taper, rate.min, rate.max),
        (
            Unit::Hertz,
            Taper::Logarithmic,
            MIN_LFO_RATE_HZ,
            MAX_LFO_RATE_HZ
        ),
        "the filter's LFO does not use the same rate range as the chorus's"
    );
    // Rule 4: the drive is nonlinear, so there is a gain after it.
    assert_eq!(spec_of(&config, "output").unit, Unit::Decibels);
    for id in ["env_attack", "env_release"] {
        assert_eq!(spec_of(&config, id).unit, Unit::Milliseconds, "{id}");
    }
    const {
        assert!(
            FILTER_MOD_OCTAVES >= 3.0,
            "a sweep of under three octaves is a wobble"
        )
    };
}

#[test]
fn the_lfo_waves_are_named_and_all_different() {
    assert_eq!(LfoWave::ALL.len(), 6);
    let config = EffectConfig::new(EffectKind::Filter);
    let wave = spec_of(&config, "lfo_wave");
    assert_eq!(wave.taper, Taper::Stepped(6));
    let labels: Vec<&str> = LfoWave::ALL.iter().map(|w| w.label()).collect();
    assert_eq!(wave.positions, labels.as_slice());
}

#[test]
fn a_fresh_filter_is_a_wire_with_nothing_moving_it() {
    let filter = FilterConfig::new();
    assert_eq!(filter.shape, FilterShape::LowPass24);
    assert_eq!(
        filter.cutoff_hz, MAX_FILTER_HZ,
        "a fresh filter is filtering"
    );
    assert_eq!(filter.resonance, 0.0);
    assert_eq!(filter.drive, 0.0);
    assert_eq!(filter.env_amount, 0.0);
    assert_eq!(filter.lfo_amount, 0.0);
    assert_eq!(filter.output_db, 0.0);
    assert_eq!(filter.mix, 1.0);
    assert!(
        !EffectKind::Filter.is_time_based(),
        "a filter replaces the signal"
    );
}

#[test]
fn the_filters_knobs_are_reachable_through_their_addresses() {
    let mut config = EffectConfig::new(EffectKind::Filter);
    for (id, value) in [
        ("shape", 4.0f32),
        ("cutoff", 800.0),
        ("resonance", 75.0),
        ("drive", 40.0),
        ("env", -80.0),
        ("env_attack", 12.0),
        ("env_release", 400.0),
        ("lfo", 60.0),
        ("lfo_rate", 3.0),
        ("lfo_wave", 5.0),
        ("output", -6.0),
    ] {
        config.set(id, value);
        let read = config
            .get(id)
            .unwrap_or_else(|| panic!("{id} is not readable"));
        assert!((read - value).abs() < 1e-3, "{id} read back as {read}");
    }
    let EffectConfig::Filter(filter) = config else {
        unreachable!()
    };
    assert_eq!(filter.shape, FilterShape::BandPass12);
    assert_eq!(filter.lfo_wave, LfoWave::SampleHold);
    assert!((filter.env_amount + 0.8).abs() < 1e-6);
}

#[test]
fn a_filter_round_trips_through_json() {
    let config = FilterConfig {
        shape: FilterShape::HighPass24,
        cutoff_hz: 320.0,
        resonance: 0.6,
        drive: 0.3,
        env_amount: -0.5,
        env_attack_ms: 8.0,
        env_release_ms: 350.0,
        lfo_amount: 0.9,
        lfo_rate_hz: 4.5,
        lfo_sync: true,
        lfo_division: NoteDivision::Sixteenth,
        lfo_wave: LfoWave::Square,
        output_db: -2.0,
        mix: 0.6,
    };
    let text = serde_json::to_string(&config).unwrap();
    let back: FilterConfig = serde_json::from_str(&text).unwrap();
    assert_eq!(back, config);
}

// --------------------------------------------------------------- presets

#[test]
fn every_recipe_the_export_tool_runs_lands_somewhere_of_its_own() {
    // The constructor presets are **files** now (`docs/flopsynth-plan.md`
    // §P.9): `cargo xtask export-factory-presets` runs these recipes once and
    // writes each result under `assets/presets/fx-*/`, and the panel reads
    // them back through the bank like every other device's.
    //
    // The recipes stay, in the position `DrumKitStyle` keeps: they are the
    // *authoring tool*, and this is the test that keeps them honest. Two
    // presets that came out identical would be two rows in the browser that
    // do the same thing, and nobody would notice from the outside.
    let distortions: Vec<DistortionConfig> = DistortionPreset::ALL
        .iter()
        .map(|preset| DistortionConfig::from_preset(*preset))
        .collect();
    for (i, one) in distortions.iter().enumerate() {
        for (j, two) in distortions.iter().enumerate().skip(i + 1) {
            assert_ne!(
                one,
                two,
                "{:?} and {:?} are the same distortion",
                DistortionPreset::ALL[i],
                DistortionPreset::ALL[j]
            );
        }
    }
    let crushes: Vec<BitcrushConfig> = BitcrushPreset::ALL
        .iter()
        .map(|preset| BitcrushConfig::from_preset(*preset))
        .collect();
    for (i, one) in crushes.iter().enumerate() {
        for two in crushes.iter().skip(i + 1) {
            assert_ne!(one, two, "two crushes are the same");
        }
    }
    let softens: Vec<SoftenConfig> = SoftenPreset::ALL
        .iter()
        .map(|preset| SoftenConfig::from_preset(*preset))
        .collect();
    for (i, one) in softens.iter().enumerate() {
        for two in softens.iter().skip(i + 1) {
            assert_ne!(one, two, "two softens are the same");
        }
    }
}

#[test]
fn a_preset_is_still_not_a_parameter() {
    // Rule 10's **first half**, which §P.9 keeps: a preset is a constructor,
    // so no effect has a knob called "preset" and no lane can sweep one. What
    // §P.6 replaced is the second half — a device remembers the *name* it was
    // loaded from now, and recognises whether it is still clean.
    for kind in EffectKind::ALL {
        let config = EffectConfig::new(kind);
        assert!(
            !config.specs().iter().any(|spec| spec.id == "preset"),
            "{kind:?} has a preset knob"
        );
    }
}

// ------------------------------------------------------------------ tune
//
// `docs/tune-plan.md` §9.4. The contract side of the pitch corrector: the
// sections a panel draws, the ids an automation lane names, the scales' notes,
// the sixteen presets, and the latency the graph compensates. The sound is
// measured in `fontelle-dsp/tests/pitch.rs`, `psola.rs` and
// `fontelle-fx/tests/tune.rs`.

#[test]
fn the_tune_is_input_scale_correction_vibrato_voice_character_then_output() {
    let config = EffectConfig::new(EffectKind::Tune);
    let names: Vec<&str> = config.sections().iter().map(|s| s.name).collect();
    assert_eq!(
        names,
        [
            "Input",
            "Scale",
            "Correction",
            "Vibrato",
            "Voice",
            "Character",
            "Output"
        ]
    );
    let counts: Vec<usize> = config.sections().iter().map(|s| s.count).collect();
    // §4: four in, sixteen for the scale (two choosers, twelve keys and the
    // two MIDI controls), five corrections, six vibrato, seven voice, the four
    // character knobs (§4.8) and the output pair.
    assert_eq!(counts, [4, 16, 5, 6, 7, 4, 2]);
    let ids: Vec<&str> = config.specs().iter().map(|spec| spec.id).collect();
    assert_eq!(
        ids,
        [
            "range",
            "mode",
            "tracking",
            "gate",
            "root",
            "scale",
            "note_c",
            "note_cs",
            "note_d",
            "note_ds",
            "note_e",
            "note_f",
            "note_fs",
            "note_g",
            "note_gs",
            "note_a",
            "note_as",
            "note_b",
            "control",
            "midi_bend",
            "retune",
            "amount",
            "humanize",
            "flex",
            "natural_vibrato",
            "vibrato_depth",
            "vibrato_rate",
            "vibrato_sync",
            "vibrato_division",
            "vibrato_onset",
            "vibrato_shape",
            "engine",
            "texture",
            "grain",
            "formant",
            "formant_follow",
            "transpose",
            "detune",
            "drive",
            "crush",
            "air",
            "width",
            "output",
            "mix",
        ]
    );
    assert_eq!(config.specs().len(), 44, "forty-three knobs and the mix");
}

#[test]
fn the_tunes_choosers_name_every_position() {
    let config = EffectConfig::new(EffectKind::Tune);
    for (id, count) in [
        ("range", 5),
        ("mode", 2),
        ("root", 12),
        ("scale", 15),
        ("control", 3),
        ("engine", 3),
        ("vibrato_shape", 2),
    ] {
        let spec = spec_of(&config, id);
        assert_eq!(
            spec.taper,
            Taper::Stepped(count),
            "{id} is a chooser of {count}"
        );
        assert_eq!(
            spec.positions.len(),
            count as usize,
            "{id} leaves a position unnamed"
        );
    }
    // And the enums behind them agree with the tables a panel reads.
    assert_eq!(TuneRange::ALL.len(), 5);
    assert_eq!(TuneMode::ALL.len(), 2);
    assert_eq!(TuneScale::ALL.len(), 15);
    assert_eq!(TuneControl::ALL.len(), 3);
    assert_eq!(TuneEngine::ALL.len(), 3);
    assert_eq!(VibratoShape::ALL.len(), 2);
    let scales = spec_of(&config, "scale");
    assert_eq!(scales.positions[0], TuneScale::Chromatic.label());
    assert_eq!(scales.positions[14], TuneScale::Custom.label());
    let roots = spec_of(&config, "root");
    assert_eq!(roots.positions[0], "C");
    assert_eq!(roots.positions[11], "B");
}

#[test]
fn the_tunes_knobs_have_the_units_and_ranges_this_plan_gives() {
    let config = EffectConfig::new(EffectKind::Tune);
    for (id, min, max, default, unit, taper) in [
        ("tracking", 0.0, 100.0, 50.0, Unit::Percent, Taper::Linear),
        ("gate", -80.0, -20.0, -50.0, Unit::Decibels, Taper::Linear),
        (
            "retune",
            MIN_TUNE_RETUNE_MS,
            MAX_TUNE_RETUNE_MS,
            20.0,
            Unit::Milliseconds,
            Taper::Logarithmic,
        ),
        ("amount", 0.0, 100.0, 100.0, Unit::Percent, Taper::Linear),
        ("humanize", 0.0, 100.0, 0.0, Unit::Percent, Taper::Linear),
        ("flex", 0.0, 100.0, 0.0, Unit::Percent, Taper::Linear),
        (
            "natural_vibrato",
            0.0,
            100.0,
            100.0,
            Unit::Percent,
            Taper::Linear,
        ),
        ("vibrato_depth", 0.0, 100.0, 0.0, Unit::None, Taper::Linear),
        (
            "vibrato_rate",
            0.1,
            12.0,
            5.5,
            Unit::Hertz,
            Taper::Logarithmic,
        ),
        (
            "vibrato_onset",
            0.0,
            1_000.0,
            200.0,
            Unit::Milliseconds,
            Taper::Linear,
        ),
        ("texture", 0.0, 100.0, 0.0, Unit::Percent, Taper::Linear),
        (
            "grain",
            MIN_TUNE_GRAIN_MS,
            MAX_TUNE_GRAIN_MS,
            25.0,
            Unit::Milliseconds,
            Taper::Logarithmic,
        ),
        ("formant", -12.0, 12.0, 0.0, Unit::None, Taper::Linear),
        (
            "formant_follow",
            0.0,
            100.0,
            0.0,
            Unit::Percent,
            Taper::Linear,
        ),
        (
            "transpose",
            -12.0,
            12.0,
            0.0,
            Unit::None,
            Taper::Stepped(25),
        ),
        ("detune", -100.0, 100.0, 0.0, Unit::None, Taper::Linear),
        ("output", -24.0, 12.0, 0.0, Unit::Decibels, Taper::Linear),
    ] {
        let spec = spec_of(&config, id);
        assert_eq!(spec.min, min, "{id}'s bottom");
        assert_eq!(spec.max, max, "{id}'s top");
        assert_eq!(spec.default, default, "{id}'s default");
        assert_eq!(spec.unit, unit, "{id}'s unit");
        assert_eq!(spec.taper, taper, "{id}'s taper");
    }
    // The mix is fully wet: a corrector replaces the signal, it does not sit
    // under it.
    assert_eq!(spec_of(&config, "mix").default, 100.0);
    assert!(!EffectKind::Tune.is_time_based());
}

#[test]
fn the_twelve_note_switches_are_switches_and_are_reachable_by_address() {
    let mut config = EffectConfig::new(EffectKind::Tune);
    for id in TUNE_NOTE_PARAMS {
        let spec = spec_of(&config, id);
        assert_eq!(spec.unit, Unit::Switch, "{id} is a switch");
        assert_eq!(spec.taper, Taper::Stepped(2));
        assert_eq!(
            spec.default, 1.0,
            "{id} opens on — a fresh tune is chromatic"
        );
    }
    // What automation writes: a lane can turn the fifth off half way through a
    // section, which is the whole reason these are parameters (§4.2).
    config.set("note_fs", 0.0);
    config.set("note_a", 0.0);
    let EffectConfig::Tune(tune) = config else {
        unreachable!()
    };
    assert_eq!(tune.notes & (1 << 6), 0, "F# is off");
    assert_eq!(tune.notes & (1 << 9), 0, "A is off");
    assert_ne!(tune.notes & 1, 0, "C was not moved by its neighbours");
    assert_eq!(tune.notes.count_ones(), 10);
}

#[test]
fn a_fresh_tune_is_a_tuner_and_not_a_wire() {
    // §4.7: the knob a person reaches for on a tuner is the retune speed, and
    // an insert that did nothing until `amount` was found would look broken.
    let tune = TuneConfig::new();
    assert_eq!(tune.scale, TuneScale::Chromatic);
    assert_eq!(tune.notes, 0x0FFF, "every key is in");
    assert_eq!(tune.root, 0);
    assert_eq!(tune.control, TuneControl::Scale);
    assert_eq!(tune.retune_ms, 20.0);
    assert_eq!(tune.amount, 1.0);
    assert_eq!(tune.humanize, 0.0);
    assert_eq!(tune.flex, 0.0);
    assert_eq!(tune.natural_vibrato, 1.0);
    assert_eq!(tune.engine, TuneEngine::Smooth);
    assert_eq!(tune.formant_follow, 0.0, "formants stay where they were");
    assert_eq!(tune.formant, 0.0);
    assert_eq!(tune.transpose, 0);
    assert_eq!(tune.mode, TuneMode::Studio);
    assert_eq!(tune.range, TuneRange::AltoTenor);
    assert_eq!(tune.vibrato_depth, 0.0);
    assert_eq!(tune.mix, 1.0);
    // The two ways to make it a wire, and they are the only two.
    assert_eq!(spec_of(&EffectConfig::Tune(tune), "amount").min, 0.0);
}

/// Written by hand rather than by calling the function it checks (§9.4).
#[test]
fn every_scale_mask_has_the_right_notes_at_every_root() {
    // Bit 0 is C, bit 11 is B — absolute pitch classes, so a mask can be read
    // against a keyboard without knowing the root.
    for (scale, at_c) in [
        (TuneScale::Chromatic, 0b1111_1111_1111u16),
        (TuneScale::Major, 0b1010_1011_0101),
        (TuneScale::NaturalMinor, 0b0101_1010_1101),
        (TuneScale::HarmonicMinor, 0b1001_1010_1101),
        (TuneScale::MelodicMinor, 0b1010_1010_1101),
        (TuneScale::Dorian, 0b0110_1010_1101),
        (TuneScale::Phrygian, 0b0101_1010_1011),
        (TuneScale::Lydian, 0b1010_1101_0101),
        (TuneScale::Mixolydian, 0b0110_1011_0101),
        (TuneScale::Locrian, 0b0101_0110_1011),
        (TuneScale::MajorPentatonic, 0b0010_1001_0101),
        (TuneScale::MinorPentatonic, 0b0100_1010_1001),
        (TuneScale::Blues, 0b0100_1110_1001),
        (TuneScale::WholeTone, 0b0101_0101_0101),
        (TuneScale::Custom, 0b1111_1111_1111),
    ] {
        assert_eq!(scale.mask(0), at_c, "{:?} at C", scale);
        // A scale is the same shape wherever it starts: rotate the notes and
        // you have the mask at that root.
        for root in 0..12u8 {
            let rotated = ((at_c as u32) << root) | ((at_c as u32) >> (12 - root as u32));
            assert_eq!(
                scale.mask(root),
                (rotated & 0x0FFF) as u16,
                "{:?} at root {root}",
                scale
            );
        }
    }
    // The two everybody checks by ear: D major has F# and C#, and it does not
    // have F.
    let d_major = TuneScale::Major.mask(2);
    assert_ne!(d_major & (1 << 6), 0, "F# is in D major");
    assert_ne!(d_major & (1 << 1), 0, "C# is in D major");
    assert_eq!(d_major & (1 << 5), 0, "F is not");
}

#[test]
fn every_tune_preset_is_somewhere_other_than_the_wire_and_than_each_other() {
    // §6's sixteen, plus the twenty-four the character section (§4.8) made
    // reachable: the bank is what somebody browses to find a *style*, and a
    // family with four new axes and no new points on them is a family whose
    // range nobody will find.
    assert_eq!(TunePreset::ALL.len(), 40, "§6's bank");
    let fresh = TuneConfig::new();
    let mut seen: Vec<(TunePreset, TuneConfig)> = Vec::new();
    for preset in TunePreset::ALL {
        let config = TuneConfig::from_preset(preset);
        assert_ne!(
            config,
            fresh,
            "{} is the state a fresh insert is already in",
            preset.label()
        );
        for (other, made) in &seen {
            assert_ne!(
                config,
                *made,
                "{} and {} are the same settings twice",
                preset.label(),
                other.label()
            );
        }
        assert!(!preset.label().is_empty());
        seen.push((preset, config));
    }
}

#[test]
fn a_tune_round_trips_through_json() {
    let mut config = TuneConfig::from_preset(TunePreset::CheapPlastic);
    config.notes = 0b0000_1010_0101;
    config.scale = TuneScale::Custom;
    config.root = 7;
    config.detune_cents = -14.0;
    let text = serde_json::to_string(&config).unwrap();
    let back: TuneConfig = serde_json::from_str(&text).unwrap();
    assert_eq!(back, config);
    // And a project written by a build with fewer knobs opens at this build's
    // defaults rather than at zero (§11).
    let sparse: TuneConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(sparse, TuneConfig::new());
}

/// §3.8's table, in samples at 48 kHz, all ten cells. The graph compensates
/// this number, so it is the one thing here that must not drift.
#[test]
fn the_latency_a_config_reports_matches_the_table_in_the_plan() {
    for (range, live, studio) in [
        (TuneRange::Soprano, 332, 664),
        (TuneRange::AltoTenor, 512, 1_024),
        (TuneRange::BaritoneBass, 832, 1_664),
        (TuneRange::Instrument, 1_232, 2_464),
        (TuneRange::Low, 1_952, 3_904),
    ] {
        for (mode, expected) in [(TuneMode::Live, live), (TuneMode::Studio, studio)] {
            let config = TuneConfig {
                range,
                mode,
                ..TuneConfig::new()
            };
            assert_eq!(
                config.latency_samples(48_000.0),
                expected,
                "{range:?} in {mode:?}"
            );
        }
    }
    // It scales with the rate, because everything in samples is derived from
    // the hertz the range is written in.
    let config = TuneConfig::new();
    assert_eq!(config.latency_samples(96_000.0), 2 * 960 + 64);
}

// ------------------------------------------- the seven of Flopsynth II §4.5

/// The seven kinds `docs/flopsynth-next.md` §4.5 adds are in the menu, and
/// the three that put copies *under* the track open half wet, like the
/// chorus and for its reason.
#[test]
fn the_seven_new_kinds_are_listed_and_the_modulated_ones_open_part_dry() {
    for kind in [
        EffectKind::Phaser,
        EffectKind::Flanger,
        EffectKind::Fold,
        EffectKind::Shifter,
        EffectKind::Hyper,
        EffectKind::Multiband,
        EffectKind::Width,
    ] {
        assert!(
            EffectKind::ALL.contains(&kind),
            "{kind:?} is not in the menu"
        );
        assert!(!kind.label().is_empty());
        assert!(!kind.takes_key() && !kind.takes_notes());
    }
    for kind in [EffectKind::Phaser, EffectKind::Flanger, EffectKind::Hyper] {
        assert!(kind.is_time_based(), "{kind:?} puts copies under the track");
    }
    for kind in [
        EffectKind::Fold,
        EffectKind::Shifter,
        EffectKind::Multiband,
        EffectKind::Width,
    ] {
        assert!(!kind.is_time_based(), "{kind:?} replaces the signal");
    }
}

#[test]
fn the_phaser_is_sweep_then_rate_then_output() {
    let config = EffectConfig::new(EffectKind::Phaser);
    let names: Vec<&str> = config.sections().iter().map(|s| s.name).collect();
    assert_eq!(names, ["Sweep", "Rate", "Output"]);
    let ids: Vec<&str> = config.specs().iter().map(|spec| spec.id).collect();
    assert_eq!(
        ids,
        [
            "stages", "centre", "depth", //
            "rate", "sync", "division", //
            "feedback", "spread", "mix",
        ]
    );
    let stages = spec_of(&config, "stages");
    assert_eq!((stages.min, stages.max), (2.0, 12.0));
    assert!(matches!(stages.taper, Taper::Stepped(_)));
    let centre = spec_of(&config, "centre");
    assert_eq!(
        (centre.unit, centre.taper),
        (Unit::Hertz, Taper::Logarithmic)
    );
    // Signed feedback, like the chorus's.
    let feedback = spec_of(&config, "feedback");
    assert!(feedback.min < 0.0 && feedback.max > 0.0);
    // Sync takes the rate from the song.
    let mut synced = fontelle_types::PhaserConfig::new();
    synced.sync = true;
    synced.division = fontelle_types::NoteDivision::Whole;
    let at_120 = synced.effective_rate_hz(120.0);
    assert!(
        (at_120 - 0.5).abs() < 1e-3,
        "a whole note at 120 is 0.5 Hz: {at_120}"
    );
}

#[test]
fn the_flanger_is_sweep_then_rate_then_output() {
    let config = EffectConfig::new(EffectKind::Flanger);
    let names: Vec<&str> = config.sections().iter().map(|s| s.name).collect();
    assert_eq!(names, ["Sweep", "Rate", "Output"]);
    let ids: Vec<&str> = config.specs().iter().map(|spec| spec.id).collect();
    assert_eq!(
        ids,
        [
            "delay",
            "depth",
            "through_zero", //
            "rate",
            "sync",
            "division", //
            "feedback",
            "spread",
            "mix",
        ]
    );
    let delay = spec_of(&config, "delay");
    assert_eq!(delay.unit, Unit::Milliseconds);
    assert!(
        delay.min <= 0.1 && delay.max >= 10.0,
        "0.1 to 10 ms: {delay:?}"
    );
    assert_eq!(spec_of(&config, "through_zero").unit, Unit::Switch);
}

#[test]
fn the_fold_the_shifter_the_hyper_and_the_width_are_one_grid_each() {
    for (kind, ids) in [
        (
            EffectKind::Fold,
            vec!["drive", "symmetry", "smooth", "output", "mix"],
        ),
        (
            EffectKind::Shifter,
            vec!["shift", "fine", "direction", "feedback", "mix"],
        ),
        (
            EffectKind::Hyper,
            vec!["voices", "detune", "spread", "window", "mix"],
        ),
        (
            EffectKind::Width,
            vec!["width", "mono_below", "mid", "side", "mix"],
        ),
    ] {
        let config = EffectConfig::new(kind);
        assert_eq!(config.sections().len(), 1, "{kind:?}");
        let got: Vec<&str> = config.specs().iter().map(|spec| spec.id).collect();
        assert_eq!(got, ids, "{kind:?}");
    }
    let shifter = EffectConfig::new(EffectKind::Shifter);
    let shift = spec_of(&shifter, "shift");
    assert_eq!(
        (shift.min, shift.max, shift.unit),
        (-5_000.0, 5_000.0, Unit::Hertz)
    );
    let direction = spec_of(&shifter, "direction");
    assert_eq!(direction.positions, ["up", "down", "both"]);
    let width = EffectConfig::new(EffectKind::Width);
    let w = spec_of(&width, "width");
    assert_eq!((w.min, w.max, w.unit), (0.0, 200.0, Unit::Percent));
    let hyper = EffectConfig::new(EffectKind::Hyper);
    let voices = spec_of(&hyper, "voices");
    assert_eq!((voices.min, voices.max), (1.0, 4.0));
}

#[test]
fn the_multiband_is_bands_then_drive_then_output() {
    let config = EffectConfig::new(EffectKind::Multiband);
    let names: Vec<&str> = config.sections().iter().map(|s| s.name).collect();
    assert_eq!(names, ["Bands", "Drive", "Output"]);
    let ids: Vec<&str> = config.specs().iter().map(|spec| spec.id).collect();
    assert_eq!(
        ids,
        [
            "low",
            "high", //
            "low_drive",
            "mid_drive",
            "high_drive", //
            "output",
            "mix",
        ]
    );
    for id in ["low_drive", "mid_drive", "high_drive"] {
        let spec = spec_of(&config, id);
        assert_eq!(
            (spec.min, spec.unit, spec.default),
            (0.0, Unit::Decibels, 0.0)
        );
    }
}

/// Every one of the seven opens as something a person can hear through:
/// the processors as a wire in their own controls, the modulated ones at
/// the chorus's half.
#[test]
fn a_fresh_one_of_the_seven_round_trips_through_json() {
    for kind in [
        EffectKind::Phaser,
        EffectKind::Flanger,
        EffectKind::Fold,
        EffectKind::Shifter,
        EffectKind::Hyper,
        EffectKind::Multiband,
        EffectKind::Width,
    ] {
        let config = EffectConfig::new(kind);
        let text = serde_json::to_string(&config).unwrap();
        let back: EffectConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(back, config, "{kind:?}");
        // And every parameter reads back what it was set to.
        let mut moved = config;
        for spec in config.specs() {
            moved.set(spec.id, spec.max);
            let got = moved.get(spec.id).unwrap();
            assert!(
                (got - spec.max).abs() < 1e-3,
                "{kind:?} {}: set {} read {got}",
                spec.id,
                spec.max
            );
        }
    }
}

#[test]
fn an_inserts_strip_label_fits_the_strip_and_its_menu_name_is_the_product() {
    // A mixer strip is 76 pixels wide (`canvas::mixer::STRIP_WIDTH`) and the
    // insert row shares that with a mix dial and the chain dots, which is why
    // a compressor says "Comp" and a distortion says "Dist". Nine characters
    // is the longest that has ever fitted — "Multiband" — and *DisgustingBeat*
    // is fourteen. It says "Disgust" on the strip and its own name everywhere
    // there is room for it: a menu row and a window title are not 76 pixels.
    for kind in EffectKind::ALL {
        let label = kind.label();
        assert!(
            !label.is_empty() && label.chars().count() <= 9,
            "{kind:?} says {label:?} on a strip, which does not fit one"
        );
        let full = kind.full_label();
        assert!(
            full.chars().count() >= label.chars().count(),
            "{kind:?}'s menu name {full:?} is shorter than its strip label {label:?}"
        );
        // Every other effect has one name and uses it in both places; the
        // abbreviation is the exception, not the rule.
        if kind != EffectKind::DisgustingBeat {
            assert_eq!(full, label, "{kind:?} grew a second name");
        }
    }
    assert_eq!(EffectKind::DisgustingBeat.full_label(), "DisgustingBeat");
}
