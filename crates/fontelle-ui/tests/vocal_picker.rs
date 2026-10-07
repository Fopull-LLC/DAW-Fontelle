//! Choosing who sings, and getting more singers: the picker grouped into
//! the Fontelle Singer's free voicebanks and the singer engines, a
//! voicebank's terms read and agreed to before it first sings, and the
//! Voicebanks page (what is installed; Find singers) — rows, layout, hits.

use fontelle_model::VocalVoice;
use fontelle_ui::canvas::{
    PageHit, SingerRow, TermsRow, VoicebankTab, singer_menu_rows, terms_menu_rows,
    voicebank_page_hit, voicebank_page_layout,
};
use fontelle_ui::document::{SingerChoice, SingerKind};
use fontelle_ui::layout::Rect;

fn bank(id: &str, name: &str, kind: SingerKind, agreed: bool) -> SingerChoice {
    SingerChoice {
        engine: "fontelle-utau".into(),
        engine_name: "UTAU".into(),
        id: id.into(),
        name: name.into(),
        languages: vec!["ja".into()],
        kind,
        terms: Some("Free for non-commercial use.".into()),
        terms_agreed: agreed,
        ..Default::default()
    }
}

fn engine_singer(id: &str, window: bool, research: bool) -> SingerChoice {
    SingerChoice {
        engine: "Some Engine".into(),
        engine_name: "Some Engine".into(),
        id: id.into(),
        name: format!("Voice {id}"),
        languages: vec!["en".into()],
        kind: SingerKind::Engine,
        has_editor: window,
        research_only: research,
        ..Default::default()
    }
}

fn labels(rows: &[(fontelle_ui::canvas::MenuEntry, SingerRow)]) -> Vec<String> {
    rows.iter().map(|(e, _)| e.label.clone()).collect()
}

#[test]
fn free_voicebanks_come_first_under_the_fontelle_singer_then_the_engines() {
    let singers = vec![
        bank("teto", "Kasane Teto", SingerKind::Utau, true),
        bank("qx", "Qixuan", SingerKind::DiffSinger, false),
        engine_singer("a", true, false),
        engine_singer("b", true, true),
    ];
    let rows = singer_menu_rows(&singers, None, &[], "");
    let text = labels(&rows);
    let fontelle = text
        .iter()
        .position(|t| t.contains("Fontelle Singer"))
        .expect("a heading for the built-in singer");
    let engines = text
        .iter()
        .position(|t| t.contains("Singer engines"))
        .expect("a heading for the engines");
    assert!(fontelle < engines);
    let teto = text.iter().position(|t| t.contains("Kasane Teto")).unwrap();
    assert!(fontelle < teto && teto < engines);
    assert!(text[teto].contains("UTAU") && text[teto].contains("Japanese"));
    let qx = text.iter().find(|t| t.contains("Qixuan")).unwrap();
    assert!(qx.contains("DiffSinger"));
    assert!(qx.contains("terms"), "unread terms are said: {qx}");
    assert!(!text[teto].contains("terms"), "agreed terms are not");
    // An engine's own heading says it has a window of its own.
    assert!(
        text.iter()
            .any(|t| t.contains("Some Engine") && t.contains("window"))
    );
    let research = rows
        .iter()
        .find(|(e, _)| e.label.contains("Voice b"))
        .unwrap();
    assert!(research.0.swatch.is_some());
    // And the ways to get more.
    assert!(rows.iter().any(|(_, r)| *r == SingerRow::InstallVoicebank));
    assert!(rows.iter().any(|(_, r)| *r == SingerRow::ManageVoicebanks));
}

#[test]
fn with_nobody_installed_the_picker_points_at_find_singers() {
    let rows = singer_menu_rows(&[], None, &[], "/x/singers");
    let text = labels(&rows);
    assert!(
        text.iter().any(|t| t.contains("No singer is installed")),
        "{text:?}"
    );
    assert!(rows.iter().any(|(_, r)| *r == SingerRow::ManageVoicebanks));
    assert!(text.iter().any(|t| t.contains("/x/singers")));
}

#[test]
fn a_voicebanks_terms_are_shown_whole_before_it_sings() {
    let long = "Line one of the terms.\n\nThis is a much longer line that goes on and on about what may and may not be done with this voice, far past the width of a menu.\nLast.";
    let rows = terms_menu_rows("Kasane Teto", long);
    assert!(rows[0].0.label.contains("Kasane Teto"));
    let lines: Vec<&str> = rows
        .iter()
        .filter(|(_, r)| *r == TermsRow::Info)
        .map(|(e, _)| e.label.as_str())
        .collect();
    assert!(lines.iter().all(|l| l.chars().count() <= 72), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("Last.")));
    let agree = rows.iter().find(|(_, r)| *r == TermsRow::Agree).unwrap();
    assert!(agree.0.enabled && agree.0.label.contains("agree"));
    assert!(rows.iter().any(|(_, r)| *r == TermsRow::Cancel));
}

#[test]
fn the_voicebanks_page_lays_out_its_rows_and_answers_presses() {
    let window = Rect::new(0.0, 0.0, 1200.0, 800.0);
    let page = voicebank_page_layout(window, VoicebankTab::Installed, 3, 0.0);
    assert!(page.frame.width < window.width && page.frame.height < window.height);
    assert_eq!(page.rows.len(), 3);
    for pair in page.rows.windows(2) {
        assert!(pair[0].frame.bottom() <= pair[1].frame.y);
    }
    let r = &page.rows[1];
    assert!(r.portrait.right() <= r.action.x);
    let centre = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
    let (x, y) = centre(r.action);
    assert_eq!(voicebank_page_hit(&page, x, y), Some(PageHit::Action(1)));
    let (x, y) = centre(r.terms);
    assert_eq!(voicebank_page_hit(&page, x, y), Some(PageHit::Terms(1)));
    let (x, y) = centre(page.tabs[1].1);
    assert_eq!(
        voicebank_page_hit(&page, x, y),
        Some(PageHit::Tab(VoicebankTab::Find))
    );
    let (x, y) = centre(page.close);
    assert_eq!(voicebank_page_hit(&page, x, y), Some(PageHit::Close));
    let (x, y) = centre(page.install);
    assert_eq!(
        voicebank_page_hit(&page, x, y),
        Some(PageHit::InstallFromFile)
    );
    assert_eq!(voicebank_page_hit(&page, 2.0, 2.0), Some(PageHit::Outside));
    // Scrolled, the rows move up and those above the list are not hit.
    let scrolled = voicebank_page_layout(window, VoicebankTab::Find, 30, 200.0);
    assert!(
        scrolled
            .rows
            .iter()
            .all(|r| r.frame.bottom() > scrolled.list.y)
    );
    assert!(
        scrolled.rows[0].index > 0,
        "the first rows have scrolled away"
    );
}

#[test]
fn choosing_a_bank_row_names_it() {
    let singers = vec![bank("teto", "Kasane Teto", SingerKind::Utau, false)];
    let current = VocalVoice::new("fontelle-utau", "teto", "ja");
    let rows = singer_menu_rows(&singers, Some(&current), &[], "");
    let chosen = rows
        .iter()
        .find(|(e, _)| e.label.contains("Kasane Teto"))
        .unwrap();
    assert!(!chosen.0.enabled, "the one singing is marked, not offered");
}
