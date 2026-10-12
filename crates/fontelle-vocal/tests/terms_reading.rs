//! Terms you can read: a bank whose terms are written in a language the
//! user does not read (Ty agreed to Kasane Teto's, which are Japanese,
//! without being able to read them) is matched to its entry in the Find
//! singers index, whose English summary is shown beside the original; and
//! a Translate link opens the original text through a translation site in
//! the browser — only when asked, and saying so.

use fontelle_vocal::catalogue::{Catalogue, CatalogueKind, match_bank, reads_as, translate_url};

/// The shipped index, so the matching is tested against the real entries.
fn shipped() -> Catalogue {
    let text = include_str!("../../../docs/singers-index.json");
    Catalogue::parse(text).expect("the shipped index reads")
}

#[test]
fn japanese_terms_are_not_english_and_english_ones_are() {
    let teto = "重音テトの音声ライブラリーは無料でご利用いただけます。\n\
                再配布・販売は禁止です。";
    assert!(!reads_as(teto, "en"));
    assert!(reads_as(teto, "ja"));
    assert!(reads_as(
        "Free to use, commercially too. Credit the voicebank.",
        "en"
    ));
    // A mostly English text with a name in kana is English.
    assert!(reads_as(
        "Kasane Teto (重音テト) is free to use for non-commercial songs. Credit TWINDRILL.",
        "en"
    ));
    assert!(!reads_as("免费使用，可商用。请注明出处。", "en"));
    assert!(!reads_as("무료로 사용할 수 있습니다.", "en"));
    // Nothing to read is nothing to translate.
    assert!(reads_as("", "en"));
    assert!(reads_as("  \n", "en"));
}

#[test]
fn an_installed_bank_finds_its_index_entry_by_folder_or_name() {
    let index = shipped();
    let teto = match_bank(
        &index,
        CatalogueKind::Utau,
        &["重音テト", "KasaneTeto_tougou_110401"],
    )
    .expect("Teto's folder names her");
    assert_eq!(teto.id, "kasane-teto.utau");
    assert!(!teto.terms.summary.is_empty());
    // The same name in another kind is another entry.
    let ritsu = match_bank(
        &index,
        CatalogueKind::DiffSinger,
        &["NamineRitsu_DiffSinger"],
    )
    .expect("Ritsu's DiffSinger bank");
    assert_eq!(ritsu.id, "namine-ritsu.diffsinger");
    let ritsu = match_bank(&index, CatalogueKind::Utau, &["NamineRitsu_VCV_1.5.1"])
        .expect("Ritsu's UTAU bank");
    assert_eq!(ritsu.id, "namine-ritsu.utau");
    // A shortened folder name still finds the singer by its first words.
    let kumi = match_bank(&index, CatalogueKind::Utau, &["Hitsuboku_Kumi_CHN"]).unwrap();
    assert_eq!(kumi.id, "hitsuboku-kumi-chn.utau");
    // A bank the index does not know is nobody.
    assert!(match_bank(&index, CatalogueKind::Utau, &["My_Own_Bank"]).is_none());
    // Nor does Canary's UTAU bank borrow the DiffSinger entry's terms.
    assert!(match_bank(&index, CatalogueKind::Utau, &["Canary_ARPAsing_1.0"]).is_none());
}

#[test]
fn an_entry_may_name_what_its_bank_installs_as() {
    let index = Catalogue::parse(
        r#"{"format": 1, "singers": [{
            "id": "a.utau", "name": "Somebody Else", "kind": "utau",
            "installs_as": ["東北きりたん", "Kiritan_Folder"],
            "terms": {"summary": "Free.", "url": "https://example.org/t"},
            "page": "https://example.org"
        }]}"#,
    )
    .unwrap();
    assert_eq!(
        match_bank(&index, CatalogueKind::Utau, &["東北きりたん"]).map(|s| s.id.as_str()),
        Some("a.utau")
    );
    assert_eq!(
        match_bank(&index, CatalogueKind::Utau, &["kiritan_folder"]).map(|s| s.id.as_str()),
        Some("a.utau")
    );
}

#[test]
fn the_translate_link_carries_the_original_text_to_a_translation_site() {
    let url = translate_url("無料です。再配布は禁止。", "en");
    assert!(url.starts_with("https://translate.google.com/"), "{url}");
    assert!(url.contains("tl=en"), "{url}");
    assert!(url.contains("sl=auto"), "{url}");
    // Percent-encoded UTF-8, nothing raw.
    assert!(url.contains("%E7%84%A1"), "無 is encoded: {url}");
    assert!(url.is_ascii());
    assert!(!url.contains(' '));
    // A long text is cut to what a URL can carry.
    let long = "利用規約".repeat(2000);
    let url = translate_url(&long, "en");
    assert!(url.len() <= 8000, "{}", url.len());
    // Never a broken escape at the cut.
    let tail = &url[url.len() - 3..];
    assert!(!tail.contains('%') || tail.starts_with('%'), "{tail}");
}
