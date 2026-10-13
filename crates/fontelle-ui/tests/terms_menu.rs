//! A voicebank's terms when they are not in the user's language: the
//! original is kept and shown, the Find singers index's English summary is
//! put above it when the bank is one the index knows, and a Translate row
//! offers the original to a translation site in the browser — saying in
//! its own words what it will do, because a menu row has no tooltip.

use fontelle_ui::canvas::{TermsRow, terms_menu_rows, terms_menu_rows_with};

const TETO: &str = "重音テトの音声ライブラリーは無料でご利用いただけます。再配布・販売は禁止です。";

#[test]
fn foreign_terms_get_the_english_summary_and_a_translate_row() {
    let rows = terms_menu_rows_with(
        "重音テト",
        TETO,
        Some("Free to download and use; the library may not be redistributed or sold."),
        true,
    );
    let labels: Vec<&str> = rows.iter().map(|(e, _)| e.label.as_str()).collect();
    let summary = labels
        .iter()
        .position(|l| l.contains("In English"))
        .expect("the summary is introduced");
    assert!(
        labels[summary + 1].starts_with("Free to download"),
        "{labels:?}"
    );
    // The original is still there, after the summary.
    let original = labels
        .iter()
        .position(|l| l.contains("無料"))
        .expect("the original is kept");
    assert!(original > summary);
    let translate = rows
        .iter()
        .find(|(_, r)| *r == TermsRow::Translate)
        .expect("a Translate row");
    assert!(translate.0.enabled);
    assert!(
        translate.0.label.contains("browser"),
        "it says where it goes: {}",
        translate.0.label
    );
    // Agree and Not now stay, last.
    let kinds: Vec<TermsRow> = rows.iter().map(|(_, r)| *r).collect();
    assert_eq!(
        kinds[kinds.len() - 2..],
        [TermsRow::Agree, TermsRow::Cancel]
    );
}

#[test]
fn terms_already_readable_are_as_they_were() {
    let english = "Free to use. Credit the voicebank.";
    assert_eq!(
        terms_menu_rows_with("Canary", english, None, false),
        terms_menu_rows("Canary", english)
    );
    // Foreign terms the index does not know still get the Translate row.
    let rows = terms_menu_rows_with("誰か", TETO, None, true);
    assert!(rows.iter().any(|(_, r)| *r == TermsRow::Translate));
    assert!(!rows.iter().any(|(e, _)| e.label.contains("In English")));
}
