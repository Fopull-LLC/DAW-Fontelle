//! Analyze Musically as a mixer insert: its saved settings
//! (`docs/analyze-musically-plan.md` §6.1).

use fontelle_types::{AnalyzeConfig, ArmMode, EffectConfig, EffectKind, PersistentId};

#[test]
fn it_is_in_the_add_menu_under_its_own_name() {
    assert!(EffectKind::ALL.contains(&EffectKind::Analyze));
    assert_eq!(EffectKind::Analyze.full_label(), "Analyze Musically");
    assert!(EffectKind::Analyze.label().chars().count() <= 7);
    assert!(!EffectKind::Analyze.is_time_based());
    assert_eq!(
        EffectConfig::new(EffectKind::Analyze).kind(),
        EffectKind::Analyze
    );
}

#[test]
fn a_new_insert_records_on_play_into_no_study_yet() {
    let EffectConfig::Analyze(config) = EffectConfig::new(EffectKind::Analyze) else {
        panic!("not an Analyze config");
    };
    assert_eq!(config.arm, ArmMode::OnPlay);
    assert_eq!(config.study, None);
    assert!(!config.post_fader);
    assert_eq!(config.mix, 1.0);
}

#[test]
fn its_controls_are_parameters_and_the_mix_stays_fully_wet() {
    let mut config = EffectConfig::new(EffectKind::Analyze);
    config.set("arm", 2.0);
    config.set("threshold", -30.0);
    config.set("release", 500.0);
    config.set("post_fader", 1.0);
    config.set("mix", 40.0);
    let EffectConfig::Analyze(analyze) = config else {
        unreachable!()
    };
    assert_eq!(analyze.arm, ArmMode::Now);
    assert_eq!(analyze.threshold_db, -30.0);
    assert_eq!(analyze.release_ms, 500.0);
    assert!(analyze.post_fader);
    // A wire blended with itself is itself only at a gain of exactly one.
    assert_eq!(config.mix(), 1.0);
    assert_eq!(config.get("arm"), Some(2.0));
}

#[test]
fn its_saved_state_round_trips_with_the_study_as_a_string() {
    let study = PersistentId::new();
    let config = EffectConfig::Analyze(AnalyzeConfig {
        arm: ArmMode::OnInput,
        threshold_db: -36.0,
        release_ms: 750.0,
        post_fader: true,
        study: Some(study),
        mix: 1.0,
    });
    let json = serde_json::to_string(&config).expect("serialises");
    assert!(json.contains(&study.0.to_string()), "{json}");
    let back: EffectConfig = serde_json::from_str(&json).expect("reads back");
    assert_eq!(back, config);
    // Written before the study or the switch existed, it still reads.
    let old: AnalyzeConfig =
        serde_json::from_str(r#"{"arm":"Now","threshold_db":-40.0,"release_ms":1000.0}"#)
            .expect("reads");
    assert_eq!(old.study, None);
    assert!(!old.post_fader);
}
