//! A knob's right-click menu: copy its value, paste it on another.
//!
//! > *"add ability to right click knobs copy value and right click and paste
//! > value of that knob either on the same knob or another knob flexibly"*
//! > (2026-10-18)
//!
//! One clipboard for every knob in the program — an effect's, an
//! instrument's, the mixer's, a clip's, Flopsynth's — holding where the knob
//! sat along its travel, so a value copied from one knob pastes onto any
//! other.

use fontelle_ui::canvas::{KnobMenuRow, knob_menu};

#[test]
fn the_menu_offers_copy_and_paste_after_automation() {
    let (entries, rows) = knob_menu("Cutoff", false, true);
    assert_eq!(entries.len(), rows.len(), "a row for every entry");
    assert_eq!(
        rows,
        vec![
            KnobMenuRow::Heading,
            KnobMenuRow::Automate,
            KnobMenuRow::CopyValue,
            KnobMenuRow::PasteValue,
        ]
    );
    assert_eq!(entries[0].label, "Cutoff");
    assert!(
        !entries[0].enabled,
        "the heading is the knob's name, greyed"
    );
    assert_eq!(entries[2].label, "Copy value");
    assert_eq!(entries[3].label, "Paste value");
    assert!(entries[3].enabled);
}

#[test]
fn paste_is_greyed_until_something_was_copied() {
    let (entries, rows) = knob_menu("Cutoff", false, false);
    let paste = rows
        .iter()
        .position(|r| *r == KnobMenuRow::PasteValue)
        .unwrap();
    assert!(!entries[paste].enabled);
}

#[test]
fn the_correctors_scale_rows_stay_and_the_value_rows_follow_them() {
    let (entries, rows) = knob_menu("Scale", true, true);
    assert_eq!(entries.len(), rows.len());
    assert_eq!(
        rows,
        vec![
            KnobMenuRow::Heading,
            KnobMenuRow::Automate,
            KnobMenuRow::CopyScale,
            KnobMenuRow::PasteScale,
            KnobMenuRow::CopyValue,
            KnobMenuRow::PasteValue,
        ]
    );
}
