//! Dry/wet per insert (TDD §13.4).
//!
//! Asked for from using the mixer: *"i should have a knob to adjust the sound
//! of the dry sound (before the plugin) and the wet sound (after the plugin
//! processes the dry sound) blending like how fl studio and other daws do
//! it."*
//!
//! # Why it lives in the effect's own config
//!
//! A mix control is not a property of *an* effect — every effect has one, and
//! parallel compression is the whole reason. It could therefore have gone on
//! `EffectSlot` beside the bypass, next to the chain rather than inside the
//! effect. It is here instead, and that buys three things at once for the cost
//! of one field per config: it is **addressable** (`ParamTarget::Insert` with
//! `"mix"`, so an automation lane can sweep it like any other parameter, TDD
//! §12), it **crosses to the audio thread** on the live channel the knobs
//! already use, and it is **saved and undone** by the machinery the parameters
//! already have. A field on the slot would have needed all three written
//! again.
//!
//! The value is stored 0..=1 — a gain, which is what the DSP multiplies by —
//! and read out as a percentage, which is what a knob says.

use fontelle_types::{CompressorConfig, EffectConfig, EffectKind, EqConfig};

#[test]
fn a_fresh_processor_is_all_wet() {
    // Because it is a *chain*, not a parallel bus: an EQ somebody just dropped
    // on a track has to be the EQ, exactly as it was before this control
    // existed. A processor *replaces* the signal — the point of a compressor
    // is the compressed track, not the compressed track under the loud one.
    for kind in EffectKind::ALL
        .into_iter()
        .filter(|kind| !kind.is_time_based())
    {
        let config = EffectConfig::new(kind);
        assert_eq!(config.mix(), 1.0, "{} opened part dry", kind.label());
    }
}

/// A delay and a reverb are the exception, and it is not a preference — it is
/// the difference between an insert that works when you add it and one that
/// makes the track vanish.
///
/// What these two produce is the *repeats* and the *tail*: `Delay::process`
/// writes echoes and no dry signal at all (see `fontelle-fx/tests/delay.rs`),
/// because `EffectNode` owns the blend for every effect and an effect mixing
/// its own dry back in would be blended twice. So an all-wet reverb insert is
/// a track replaced by its own reverb tail, with the sound that caused it
/// gone. Every other DAW opens these part dry for the same reason.
///
/// Which leaves the rule sharper than "all effects open wet": **an effect
/// opens at the default its own spec declares**, and the specs say what each
/// kind is for.
#[test]
fn a_fresh_time_based_effect_opens_with_the_dry_signal_under_it() {
    for kind in EffectKind::ALL
        .into_iter()
        .filter(|kind| kind.is_time_based())
    {
        let config = EffectConfig::new(kind);
        assert!(
            config.mix() > 0.0 && config.mix() < 1.0,
            "{} opened at {}, which is either inaudible or the whole track",
            kind.label(),
            config.mix()
        );
    }
}

#[test]
fn a_fresh_effect_opens_at_the_mix_its_own_spec_declares() {
    // The two above are the *values*; this is the rule they are instances of.
    // A default in the spec table that the constructor disagrees with is a
    // knob that jumps the first time anybody touches it.
    for kind in EffectKind::ALL {
        let config = EffectConfig::new(kind);
        let spec = config
            .specs()
            .iter()
            .find(|spec| spec.id == "mix")
            .expect("every effect has one");
        assert!(
            (config.mix() * 100.0 - spec.default).abs() < 1e-4,
            "{} opens at {}% and its spec says {}%",
            kind.label(),
            config.mix() * 100.0,
            spec.default
        );
    }
}

/// Which effects sit *under* the track rather than replacing it. One list, on
/// the kind itself, rather than a `matches!` in each of the three places that
/// needs to know — the constructor's default, this test, and the read-out.
#[test]
fn the_time_based_effects_are_the_delay_and_the_reverb() {
    assert!(EffectKind::Delay.is_time_based());
    assert!(EffectKind::Reverb.is_time_based());
    assert!(!EffectKind::Eq.is_time_based());
    assert!(!EffectKind::Compressor.is_time_based());
    assert!(!EffectKind::Distortion.is_time_based());
}

#[test]
fn every_effect_has_the_control() {
    // Every one, and by the same name: a knob that exists on some effects is
    // a knob nobody can rely on, and an automation lane that names `mix` must
    // find one whatever the slot holds.
    for kind in EffectKind::ALL {
        let mut config = EffectConfig::new(kind);
        config.set("mix", 40.0);
        assert!(
            (config.mix() - 0.4).abs() < 1e-6,
            "{} did not take a mix",
            kind.label()
        );
        assert_eq!(
            config.get("mix"),
            Some(40.0),
            "{} reads its mix back in percent",
            kind.label()
        );
    }
}

#[test]
fn the_control_is_addressable_like_every_other_parameter() {
    // §8.2's payoff: a parameter in the spec table gets a knob, a read-out and
    // an automation lane without any of the three being written for it.
    for kind in EffectKind::ALL {
        let config = EffectConfig::new(kind);
        let spec = config
            .specs()
            .iter()
            .find(|spec| spec.id == "mix")
            .unwrap_or_else(|| panic!("{} has no mix parameter", kind.label()));
        assert_eq!(spec.min, 0.0);
        assert_eq!(spec.max, 100.0);
        assert_eq!(spec.unit, fontelle_types::Unit::Percent);
    }
}

#[test]
fn a_lane_sweeping_it_runs_from_dry_to_wet() {
    let mut config = EffectConfig::new(EffectKind::Eq);
    config.set_normalised("mix", 0.0);
    assert_eq!(
        config.mix(),
        0.0,
        "the bottom of the lane is the dry signal"
    );
    config.set_normalised("mix", 1.0);
    assert_eq!(config.mix(), 1.0, "and the top is the effect");
    config.set_normalised("mix", 0.5);
    assert!((config.mix() - 0.5).abs() < 1e-6);
}

#[test]
fn a_mix_outside_the_range_is_clamped_rather_than_believed() {
    let mut config = EffectConfig::new(EffectKind::Compressor);
    config.set("mix", 400.0);
    assert_eq!(config.mix(), 1.0);
    config.set("mix", -50.0);
    assert_eq!(config.mix(), 0.0);
}

#[test]
fn a_project_written_before_the_control_existed_opens_fully_wet() {
    // INVARIANT: an old project sounds the way it did. A missing `mix` is not
    // a mix of zero — that would silently switch every effect in every saved
    // song out of its chain.
    let old = r#"{"bands":[]}"#;
    let eq: EqConfig = serde_json::from_str(&old.replace(
        "\"bands\":[]",
        &format!(
            "\"bands\":{}",
            serde_json::to_string(&EqConfig::new().bands).unwrap()
        ),
    ))
    .expect("an EQ written without a mix must still load");
    assert_eq!(eq.mix, 1.0);

    let comp: CompressorConfig = serde_json::from_str(
        r#"{"threshold_db":-18.0,"ratio":1.0,"attack_ms":10.0,"release_ms":100.0,
            "knee_db":6.0,"makeup_db":0.0,"auto_makeup":false,"detection":"Peak"}"#,
    )
    .expect("a compressor written without a mix must still load");
    assert_eq!(comp.mix, 1.0);
}
