//! The limiter as an insert (`docs/effects-catalogue.md`; the effect the
//! handoff had down as "the DSP exists, an afternoon").
//!
//! A brickwall on a track, and — because it has a detector like the compressor
//! — a **ducker** when its detector is keyed from another track. The Ducking
//! preset is the one the sidechain workflow reaches for: put it on the bass,
//! key it from the kick, and the kick pushes the bass down under it.

use fontelle_types::{EffectConfig, EffectKind, LimiterConfig, LimiterPreset};

#[test]
fn the_limiter_is_one_of_the_effects_the_menu_offers() {
    assert!(
        EffectKind::ALL.contains(&EffectKind::Limiter),
        "the limiter has to be in the add menu's list"
    );
    assert_eq!(EffectKind::Limiter.label(), "Limit");
}

#[test]
fn the_limiter_has_a_detector_so_it_can_be_keyed() {
    // The whole point of the ducking use: its detector listens to another
    // track. The compressor and gate are the other two that do.
    assert!(
        EffectKind::Limiter.takes_key(),
        "a ducker keyed from the kick needs a detector"
    );
}

#[test]
fn a_fresh_limiter_is_a_near_transparent_brickwall() {
    // An effect somebody just dropped on a track must not change the sound
    // until they touch it: a fresh ceiling sits just under full scale.
    let EffectConfig::Limiter(config) = EffectConfig::new(EffectKind::Limiter) else {
        panic!("new(Limiter) is a limiter");
    };
    assert!(
        config.ceiling_db > -1.0 && config.ceiling_db <= 0.0,
        "a fresh limiter's ceiling is essentially 0 dBFS, not a duck: {}",
        config.ceiling_db
    );
}

#[test]
fn every_limiter_parameter_is_reachable_by_its_address() {
    // The list automation works from — a knob a lane cannot reach is the
    // silent defect the whole spec table exists to prevent.
    let config = EffectConfig::new(EffectKind::Limiter);
    for spec in config.specs() {
        assert!(
            config.get(spec.id).is_some(),
            "the limiter's {:?} parameter has no reader",
            spec.id
        );
    }
    // The two it is made of, plus the mix every effect has.
    for id in ["ceiling", "release", "mix"] {
        assert!(
            config.specs().iter().any(|s| s.id == id),
            "the limiter is missing its {id} parameter"
        );
    }
}

#[test]
fn the_ducking_preset_sets_a_low_ceiling_so_the_key_pushes_through_it() {
    // Ducking is the ceiling low enough that the kick's peaks are over it: the
    // overage is how far the track is pushed down. A brickwall-at-0 preset
    // would never duck.
    let config = LimiterConfig::from_preset(LimiterPreset::Ducking);
    assert!(
        config.ceiling_db <= -6.0,
        "the ducking preset needs a ceiling the key exceeds: {}",
        config.ceiling_db
    );
}

#[test]
fn the_master_preset_is_a_transparent_brickwall() {
    let config = LimiterConfig::from_preset(LimiterPreset::Master);
    assert!(
        config.ceiling_db > -2.0,
        "a mastering ceiling sits just under full scale: {}",
        config.ceiling_db
    );
}
