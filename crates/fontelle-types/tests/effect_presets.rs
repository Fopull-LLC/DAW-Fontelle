//! Every built-in effect ships with presets that are worth having.
//!
//! > *"please also ensure that every built in effect plugin has a bunch of
//! > presets that will be generally useful in a wide variety of situations
//! > especially the compressor which im noticing has no presets right now."*
//!
//! Eight of the twelve effects had none: they shipped as the wire and a
//! panel. These are the recipes for them, held to the same rules the
//! distortion's seven are (`effect_families.rs`): every preset is somewhere
//! other than the wire and than every other preset, is labelled, and sets
//! every knob to a value the knob can actually hold — a preset outside a
//! parameter's range is one the panel would draw wrong and the automation
//! could not reach.

use fontelle_types::{
    ChorusConfig, ChorusPreset, CompressorConfig, CompressorPreset, DelayConfig, DelayPreset,
    EffectConfig, EffectKind, EqConfig, EqPreset, FilterConfig, FilterPreset, GateConfig,
    GatePreset, ReverbConfig, ReverbPreset, UtilityConfig, UtilityPreset,
};

/// The rules every family is held to.
fn a_family_of_presets(kind: EffectKind, configs: &[(String, EffectConfig)], at_least: usize) {
    assert!(
        configs.len() >= at_least,
        "{kind:?} has {} presets; a bank worth opening has at least {at_least}",
        configs.len()
    );
    let wire = EffectConfig::new(kind);
    for (i, (label, config)) in configs.iter().enumerate() {
        assert_eq!(config.kind(), kind);
        assert!(!label.is_empty(), "{kind:?} preset {i} has no name");
        assert_ne!(*config, wire, "{kind:?} {label:?} is the wire");
        for (other_label, other) in &configs[..i] {
            assert_ne!(
                config, other,
                "{kind:?} {label:?} duplicates {other_label:?}"
            );
            assert_ne!(
                label, other_label,
                "{kind:?} has two presets called {label:?}"
            );
        }
        // Every knob within its own range: what the panel draws and the
        // automation reaches is bounded by the spec, so a preset outside it
        // is a preset the window cannot show honestly.
        for spec in config.specs() {
            let value = config
                .get(spec.id)
                .unwrap_or_else(|| panic!("{kind:?} {label:?}: no value for {}", spec.id));
            assert!(
                value >= spec.min - 1e-3 && value <= spec.max + 1e-3,
                "{kind:?} {label:?}: {} = {value} is outside {}..={}",
                spec.id,
                spec.min,
                spec.max
            );
        }
    }
}

#[test]
fn the_compressor_has_a_real_bank() {
    // The one named in the report. Enough for the things a compressor is
    // reached for: a vocal, a drum bus, a kick, a bass, parallel crush, a
    // gentle master, and the pump a sidechain wants.
    let configs: Vec<(String, EffectConfig)> = CompressorPreset::ALL
        .iter()
        .map(|p| {
            (
                p.label().to_string(),
                EffectConfig::Compressor(CompressorConfig::from_preset(*p)),
            )
        })
        .collect();
    a_family_of_presets(EffectKind::Compressor, &configs, 12);
    // Each of them compresses: a ratio of one is a wire whatever else is set.
    for p in CompressorPreset::ALL {
        assert!(
            CompressorConfig::from_preset(p).ratio > 1.0,
            "{p:?} does not compress"
        );
    }
    // And the parallel one is the parallel one — mostly dry, hard squash.
    let parallel = CompressorConfig::from_preset(CompressorPreset::ParallelCrush);
    assert!(parallel.mix < 0.6 && parallel.ratio >= 8.0);
}

#[test]
fn the_gate_has_a_bank() {
    let configs: Vec<_> = GatePreset::ALL
        .iter()
        .map(|p| {
            (
                p.label().to_string(),
                EffectConfig::Gate(GateConfig::from_preset(*p)),
            )
        })
        .collect();
    a_family_of_presets(EffectKind::Gate, &configs, 8);
    // A gate preset gates: its threshold is above the floor, where the
    // default parks it so a fresh gate lets everything through.
    for p in GatePreset::ALL {
        assert!(
            GateConfig::from_preset(p).threshold_db > fontelle_types::GATE_FLOOR_DB,
            "{p:?} is open"
        );
    }
}

#[test]
fn the_chorus_has_a_bank() {
    let configs: Vec<_> = ChorusPreset::ALL
        .iter()
        .map(|p| {
            (
                p.label().to_string(),
                EffectConfig::Chorus(ChorusConfig::from_preset(*p)),
            )
        })
        .collect();
    a_family_of_presets(EffectKind::Chorus, &configs, 8);
    // Both machines are represented (`ChorusMode`): a bank with no ensemble
    // in it is half the effect.
    assert!(
        ChorusPreset::ALL.iter().any(|p| {
            ChorusConfig::from_preset(*p).mode == fontelle_types::ChorusMode::Ensemble
        })
    );
    // And a flanger: feedback is what makes one, and it is the family's
    // other half.
    assert!(
        ChorusPreset::ALL
            .iter()
            .any(|p| ChorusConfig::from_preset(*p).feedback.abs() > 0.5)
    );
}

#[test]
fn the_delay_has_a_bank() {
    let configs: Vec<_> = DelayPreset::ALL
        .iter()
        .map(|p| {
            (
                p.label().to_string(),
                EffectConfig::Delay(DelayConfig::from_preset(*p)),
            )
        })
        .collect();
    a_family_of_presets(EffectKind::Delay, &configs, 10);
    // Tempo-synced and free-running both, and a ping-pong.
    let all: Vec<DelayConfig> = DelayPreset::ALL
        .iter()
        .map(|p| DelayConfig::from_preset(*p))
        .collect();
    assert!(all.iter().any(|d| d.sync) && all.iter().any(|d| !d.sync));
    assert!(all.iter().any(|d| d.ping_pong));
}

#[test]
fn the_reverb_has_a_bank() {
    let configs: Vec<_> = ReverbPreset::ALL
        .iter()
        .map(|p| {
            (
                p.label().to_string(),
                EffectConfig::Reverb(ReverbConfig::from_preset(*p)),
            )
        })
        .collect();
    a_family_of_presets(EffectKind::Reverb, &configs, 10);
    // From a closet to a cathedral: the decays span an order of magnitude.
    let decays: Vec<f32> = ReverbPreset::ALL
        .iter()
        .map(|p| ReverbConfig::from_preset(*p).decay_s)
        .collect();
    let (min, max) = decays
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), d| (lo.min(*d), hi.max(*d)));
    assert!(max / min >= 10.0, "{min}..{max}");
}

#[test]
fn the_filter_has_a_bank() {
    let configs: Vec<_> = FilterPreset::ALL
        .iter()
        .map(|p| {
            (
                p.label().to_string(),
                EffectConfig::Filter(FilterConfig::from_preset(*p)),
            )
        })
        .collect();
    a_family_of_presets(EffectKind::Filter, &configs, 10);
    let all: Vec<FilterConfig> = FilterPreset::ALL
        .iter()
        .map(|p| FilterConfig::from_preset(*p))
        .collect();
    // The three things a filter effect is for: a static tone, a wah that
    // follows the playing, and a sweep on a clock.
    assert!(all.iter().any(|f| f.env_amount.abs() > 0.3));
    assert!(all.iter().any(|f| f.lfo_amount > 0.3 && f.lfo_sync));
    assert!(
        all.iter()
            .any(|f| f.env_amount == 0.0 && f.lfo_amount == 0.0)
    );
}

#[test]
fn the_eq_has_a_bank() {
    let configs: Vec<_> = EqPreset::ALL
        .iter()
        .map(|p| {
            (
                p.label().to_string(),
                EffectConfig::Eq(EqConfig::from_preset(*p)),
            )
        })
        .collect();
    a_family_of_presets(EffectKind::Eq, &configs, 10);
    // Every EQ preset does something audible: at least one band is on and
    // not a zero-gain bell.
    for p in EqPreset::ALL {
        assert!(
            EqConfig::from_preset(p)
                .bands
                .iter()
                .any(|b| b.is_audible()),
            "{p:?} is flat"
        );
    }
}

#[test]
fn the_utility_has_a_bank() {
    let configs: Vec<_> = UtilityPreset::ALL
        .iter()
        .map(|p| {
            (
                p.label().to_string(),
                EffectConfig::Utility(UtilityConfig::from_preset(*p)),
            )
        })
        .collect();
    a_family_of_presets(EffectKind::Utility, &configs, 8);
}

// ------------------------------------------- the seven of Flopsynth II §4.5
//
// Phaser, flanger, wavefolder, frequency shifter, hyper, multiband
// distortion and width (`docs/flopsynth-next.md` §4.5). Each ships a bank
// from the day it exists, held to the same rules as the eight above.

macro_rules! bank_of {
    ($preset:ident, $config:ident, $variant:ident) => {
        fontelle_types::$preset::ALL
            .iter()
            .map(|p| {
                (
                    p.label().to_string(),
                    EffectConfig::$variant(fontelle_types::$config::from_preset(*p)),
                )
            })
            .collect::<Vec<_>>()
    };
}

#[test]
fn the_phaser_has_a_bank() {
    let configs = bank_of!(PhaserPreset, PhaserConfig, Phaser);
    a_family_of_presets(EffectKind::Phaser, &configs, 6);
    // Both ends of the stage count, and a synced one.
    let stages: Vec<u32> = configs
        .iter()
        .filter_map(|(_, c)| match c {
            EffectConfig::Phaser(p) => Some(p.stages),
            _ => None,
        })
        .collect();
    assert!(stages.iter().any(|s| *s <= 4) && stages.iter().any(|s| *s >= 10));
    assert!(
        configs
            .iter()
            .any(|(_, c)| matches!(c, EffectConfig::Phaser(p) if p.sync))
    );
}

#[test]
fn the_flanger_has_a_bank() {
    let configs = bank_of!(FlangerPreset, FlangerConfig, Flanger);
    a_family_of_presets(EffectKind::Flanger, &configs, 6);
    // The hollow one (negative feedback) and the through-zero one.
    assert!(
        configs
            .iter()
            .any(|(_, c)| matches!(c, EffectConfig::Flanger(f) if f.feedback < -0.5))
    );
    assert!(
        configs
            .iter()
            .any(|(_, c)| matches!(c, EffectConfig::Flanger(f) if f.through_zero))
    );
}

#[test]
fn the_fold_has_a_bank() {
    let configs = bank_of!(FoldPreset, FoldConfig, Fold);
    a_family_of_presets(EffectKind::Fold, &configs, 6);
    assert!(
        configs
            .iter()
            .any(|(_, c)| matches!(c, EffectConfig::Fold(f) if f.symmetry.abs() > 0.3))
    );
}

#[test]
fn the_shifter_has_a_bank() {
    let configs = bank_of!(ShifterPreset, ShifterConfig, Shifter);
    a_family_of_presets(EffectKind::Shifter, &configs, 6);
    // A barber-pole (feedback with a small shift) and a down one.
    assert!(
        configs
            .iter()
            .any(|(_, c)| matches!(c, EffectConfig::Shifter(s) if s.feedback > 0.5))
    );
    assert!(configs.iter().any(|(_, c)| matches!(c, EffectConfig::Shifter(s) if s.direction == fontelle_types::ShiftDirection::Down)));
}

#[test]
fn the_hyper_has_a_bank() {
    let configs = bank_of!(HyperPreset, HyperConfig, Hyper);
    a_family_of_presets(EffectKind::Hyper, &configs, 6);
    assert!(
        configs
            .iter()
            .any(|(_, c)| matches!(c, EffectConfig::Hyper(h) if h.voices == 4))
    );
}

#[test]
fn the_multiband_has_a_bank() {
    let configs = bank_of!(MultibandPreset, MultibandConfig, Multiband);
    a_family_of_presets(EffectKind::Multiband, &configs, 6);
    // One that drives the low band alone, and one the top alone.
    assert!(configs.iter().any(|(_, c)| matches!(c, EffectConfig::Multiband(m) if m.low_drive_db > 6.0 && m.high_drive_db < 1.0)));
    assert!(configs.iter().any(|(_, c)| matches!(c, EffectConfig::Multiband(m) if m.high_drive_db > 6.0 && m.low_drive_db < 1.0)));
}

#[test]
fn the_width_has_a_bank() {
    let configs = bank_of!(WidthPreset, WidthConfig, Width);
    a_family_of_presets(EffectKind::Width, &configs, 6);
    assert!(
        configs
            .iter()
            .any(|(_, c)| matches!(c, EffectConfig::Width(w) if w.mono_below_hz > 60.0))
    );
}
