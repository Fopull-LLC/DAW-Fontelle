//! The export prompt: what Ctrl+E asks before it renders.
//!
//! > *"when i click export it prompts me with the export options so i can
//! > chose things like time selection, whole song, keep things like reverb
//! > tail or cut short, etc."*
//!
//! One menu, four answers — each a stretch and a tail — under a heading,
//! the way the row's render prompt is built. The two answers that need a
//! time selection are **shown greyed** without one rather than left out,
//! so the menu says what a selection would let you do.

use fontelle_ui::canvas::{export_menu_choice, export_menu_entries};
use fontelle_ui::document::{ExportOptions, ExportRange, ExportTail};

#[test]
fn the_prompt_offers_the_whole_song_and_the_selection_each_with_or_without_the_tail() {
    let entries = export_menu_entries(true);
    let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
    assert_eq!(labels.len(), 5, "a heading and four answers: {labels:?}");
    assert!(!entries[0].enabled, "the heading is not an answer");
    for (index, entry) in entries.iter().enumerate().skip(1) {
        assert!(
            entry.enabled,
            "row {index} is greyed with a selection there"
        );
    }
    assert_eq!(
        export_menu_choice(true, 1),
        Some(ExportOptions {
            range: ExportRange::WholeSong,
            tail: ExportTail::Keep
        })
    );
    assert_eq!(
        export_menu_choice(true, 2),
        Some(ExportOptions {
            range: ExportRange::WholeSong,
            tail: ExportTail::Cut
        })
    );
    assert_eq!(
        export_menu_choice(true, 3),
        Some(ExportOptions {
            range: ExportRange::Selection,
            tail: ExportTail::Keep
        })
    );
    assert_eq!(
        export_menu_choice(true, 4),
        Some(ExportOptions {
            range: ExportRange::Selection,
            tail: ExportTail::Cut
        })
    );
    assert_eq!(
        export_menu_choice(true, 0),
        None,
        "the heading answers nothing"
    );
    assert_eq!(export_menu_choice(true, 5), None);
}

#[test]
fn without_a_selection_the_selection_rows_are_shown_greyed_and_answer_nothing() {
    let entries = export_menu_entries(false);
    assert_eq!(entries.len(), 5, "still listed, so the option is known");
    assert!(entries[1].enabled && entries[2].enabled);
    assert!(!entries[3].enabled && !entries[4].enabled);
    assert_eq!(export_menu_choice(false, 3), None);
    assert_eq!(export_menu_choice(false, 4), None);
    assert!(export_menu_choice(false, 1).is_some());
}

#[test]
fn every_row_says_which_stretch_and_what_happens_to_the_tail() {
    // A row that only said "Keep tail" would leave which stretch to memory.
    for entry in export_menu_entries(true).iter().skip(1) {
        let label = entry.label.to_lowercase();
        assert!(
            label.contains("whole song") || label.contains("selection"),
            "{label:?} does not say which stretch"
        );
        assert!(
            label.contains("tail") || label.contains("cut"),
            "{label:?} does not say what happens at the end"
        );
    }
}
