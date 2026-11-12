//! Typing into the built-in instrument and effect menus (Ty, 2026-11-12:
//! *"right now you can search when looking for presets or plugins and
//! instruments but when youre using a built in instrument or effect you
//! cannot search for it you have to look through the menu make it so
//! searching works in all of these that users would expect"*).

use fontelle_ui::canvas::{EffectRow, effect_menu_rows, instrument_menu_rows, narrowed_rows};

#[test]
fn nothing_typed_is_the_menu_as_it_was_with_the_hint() {
    let rows = effect_menu_rows(&[], &[]);
    let narrowed = narrowed_rows(rows.clone(), "");
    assert_eq!(narrowed.len(), rows.len());
    assert!(
        narrowed[0].0.label.contains("type to search"),
        "{}",
        narrowed[0].0.label
    );
    for (a, b) in rows.iter().zip(&narrowed).skip(1) {
        assert_eq!(a.0.label, b.0.label);
    }
}

#[test]
fn typing_keeps_only_what_matches_by_name() {
    let narrowed = narrowed_rows(effect_menu_rows(&[], &[]), "rev");
    assert!(
        narrowed[0].0.label.contains("rev"),
        "the heading says what was typed"
    );
    let picks: Vec<&str> = narrowed[1..]
        .iter()
        .filter(|(e, _)| e.enabled)
        .map(|(e, _)| e.label.as_str())
        .collect();
    assert!(!picks.is_empty());
    assert!(
        picks.iter().all(|l| l.to_lowercase().contains("rev")),
        "{picks:?}"
    );
    assert!(
        narrowed[1..].iter().all(|(e, row)| e.enabled
            && !e.separator
            && matches!(row, EffectRow::Builtin(_) | EffectRow::Plugin(_))),
        "no group headings or rules between results"
    );
    // Case does not matter.
    assert_eq!(
        narrowed_rows(effect_menu_rows(&[], &[]), "REV").len(),
        narrowed.len()
    );
}

#[test]
fn nothing_matching_says_so() {
    let narrowed = narrowed_rows(effect_menu_rows(&[], &[]), "zzzzqq");
    assert_eq!(narrowed.len(), 2);
    assert!(!narrowed[1].0.enabled);
    assert!(narrowed[1].0.label.contains("nothing matches"));
}

#[test]
fn the_instruments_narrow_the_same_way() {
    let narrowed = narrowed_rows(instrument_menu_rows(None, &[], &[]), "flop");
    let picks: Vec<&str> = narrowed[1..]
        .iter()
        .map(|(e, _)| e.label.as_str())
        .collect();
    assert!(picks.iter().any(|l| l.contains("Flopsynth")), "{picks:?}");
    assert!(picks.iter().all(|l| l.to_lowercase().contains("flop")));
}

/// Seen typing "obx" in the real window: Obxd was found and OB-Xf was not,
/// for its hyphen. A search reads letters and digits, not punctuation.
#[test]
fn a_search_ignores_punctuation_and_spaces() {
    use fontelle_ui::canvas::menu_matches;
    assert!(menu_matches("OB-Xf — Surge Synth Team", "obx"));
    assert!(menu_matches("Fontelle Test Gain", "test gain"));
    assert!(menu_matches("Fontelle Test Gain", "testgain"));
    assert!(menu_matches("Vocal track from a song file…", "song file"));
    assert!(!menu_matches("Reverb", "rvb"));
    assert!(menu_matches("Anything", "  "));
}
