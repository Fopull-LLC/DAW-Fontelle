//! Chinese and Korean UTAU banks: lyrics in Chinese characters sung on
//! pinyin aliases, hangul on hangul aliases (CV, with vowel-to-consonant
//! and coda units when the bank has them), words found across notes.

use std::collections::BTreeMap;
use std::path::Path;

use fontelle_sing::{Phrase, PhraseNote};
use fontelle_utau::bank::Voicebank;
use fontelle_utau::oto::OtoEntry;
use fontelle_utau::phonemize::{self, Phonemizer, PlannedUnit};

fn bank(aliases: &[&str]) -> Voicebank {
    let entries = aliases
        .iter()
        .enumerate()
        .map(|(i, a)| OtoEntry {
            alias: a.to_string(),
            file: format!("{i}.wav").into(),
            folder: String::new(),
            offset: 0.0,
            consonant: 160.0,
            cutoff: -500.0,
            preutterance: 120.0,
            overlap: 15.0,
        })
        .collect();
    Voicebank::from_parts(Path::new("/b"), "b", entries, vec![])
}

fn phrase(language: &str, notes: &[(f64, f64, &str)]) -> Phrase {
    Phrase {
        voice: "v".into(),
        language: language.into(),
        notes: notes
            .iter()
            .map(|&(s, d, l)| PhraseNote::new(s, d, 60.0, l))
            .collect(),
        curves: BTreeMap::new(),
        extra_curves: BTreeMap::new(),
        settings: BTreeMap::new(),
        state: None,
        start: 0.0,
        note_ids: Vec::new(),
    }
}

fn units(u: &[PlannedUnit]) -> Vec<(String, usize, i64, bool)> {
    u.iter()
        .map(|u| {
            (
                u.alias.clone(),
                u.note,
                (u.anchor * 1000.0).round() as i64,
                u.tail,
            )
        })
        .collect()
}

fn u(alias: &str, note: usize, anchor_ms: i64) -> (String, usize, i64, bool) {
    (alias.to_string(), note, anchor_ms, false)
}

fn tail(alias: &str, note: usize, anchor_ms: i64) -> (String, usize, i64, bool) {
    (alias.to_string(), note, anchor_ms, true)
}

#[test]
fn chinese_and_korean_banks_are_told_by_their_aliases() {
    let pinyin = [
        "a", "ai", "ba", "bi", "zhi", "chi", "shi", "ri", "zi", "ci", "si", "xi", "qi", "ji",
        "zha", "cha", "sha", "xia", "qia", "jia", "zhang", "chang", "shang", "xiang", "qiang",
        "jiang", "zhong", "chong", "xiong", "qiong", "jiong", "nv", "lv", "xue", "que", "jue",
        "ni", "hao",
    ];
    assert_eq!(phonemize::detect(&bank(&pinyin)), Phonemizer::ZhCvvc);
    let hangul = [
        "가", "나", "다", "라", "마", "바", "사", "아", "자", "차", "카", "타", "파", "하", "거",
        "너", "더", "러", "머", "버", "서", "어", "저", "처", "커", "터", "퍼", "허", "고", "노",
        "도", "로", "모", "보", "소",
    ];
    assert_eq!(phonemize::detect(&bank(&hangul)), Phonemizer::KoCvvc);
    assert_eq!(Phonemizer::ZhCvvc.language(), "zh");
    assert_eq!(Phonemizer::KoCvvc.language(), "ko");
    // Appended: a saved setting keeps its meaning.
    assert_eq!(Phonemizer::ZhCvvc.setting_value(), 7.0);
    assert_eq!(Phonemizer::KoCvvc.setting_value(), 8.0);
}

#[test]
fn chinese_characters_sing_on_pinyin_aliases() {
    let b = bank(&["ni", "hao", "wo", "ai"]);
    let p = phrase("zh", &[(0.0, 0.5, "你"), (0.5, 0.5, "好")]);
    let plan = phonemize::plan(&b, &p, Phonemizer::ZhCvvc);
    assert_eq!(units(&plan.units), [u("ni", 0, 0), u("hao", 1, 500)]);
    assert!(plan.guessed.iter().all(|g| !g));
    // Pinyin typed as it is, and two characters on one note split it.
    let p = phrase("zh", &[(0.0, 0.5, "wo"), (0.5, 1.0, "爱你")]);
    let plan = phonemize::plan(&b, &p, Phonemizer::ZhCvvc);
    assert_eq!(
        units(&plan.units),
        [u("wo", 0, 0), u("ai", 1, 500), u("ni", 1, 1000)]
    );
}

#[test]
fn a_chinese_cvvc_bank_adds_vowel_to_consonant_and_endings() {
    let b = bank(&[
        "- ni", "ni", "hao", "i h", "ao R", "xing", "hang", "yin", "in h",
    ]);
    let p = phrase("zh", &[(0.0, 0.5, "你"), (0.5, 0.5, "好")]);
    let plan = phonemize::plan(&b, &p, Phonemizer::ZhCvvc);
    assert_eq!(
        units(&plan.units),
        [
            u("- ni", 0, 0),
            u("i h", 0, 380),
            u("hao", 1, 500),
            tail("ao R", 1, 1000)
        ]
    );
    // 银行 a character a note is still the word: hang, not xing.
    let p = phrase("zh", &[(0.0, 0.5, "银"), (0.5, 0.5, "行")]);
    let plan = phonemize::plan(&b, &p, Phonemizer::ZhCvvc);
    assert_eq!(
        units(&plan.units),
        [u("yin", 0, 0), u("in h", 0, 380), u("hang", 1, 500)]
    );
}

#[test]
fn hangul_sings_open_syllables_codas_and_vowel_to_consonant() {
    let b = bank(&[
        "- 사", "사", "라", "해", "a r", "a ng", "ae R", "으", "마", "a k",
    ]);
    let p = phrase(
        "ko",
        &[(0.0, 0.5, "사"), (0.5, 0.5, "랑"), (1.0, 0.5, "해")],
    );
    let plan = phonemize::plan(&b, &p, Phonemizer::KoCvvc);
    assert_eq!(
        units(&plan.units),
        [
            u("- 사", 0, 0),
            u("a r", 0, 380),
            u("라", 1, 500),
            // The coda before the note ends.
            u("a ng", 1, 880),
            u("해", 2, 1000),
            tail("ae R", 2, 1500),
        ]
    );
    // 음악 a syllable a note: the ㅁ is sung on the second note (마).
    let p = phrase("ko", &[(0.0, 0.5, "음"), (0.5, 0.5, "악")]);
    let plan = phonemize::plan(&b, &p, Phonemizer::KoCvvc);
    assert_eq!(
        units(&plan.units),
        [u("으", 0, 0), u("마", 1, 500), u("a k", 1, 880)]
    );
    assert!(plan.guessed.iter().all(|g| !g));
}

#[test]
fn a_bank_with_whole_syllables_sings_them_whole() {
    let b = bank(&["한", "글", "하", "그"]);
    let p = phrase("ko", &[(0.0, 0.5, "한"), (0.5, 0.5, "글")]);
    let plan = phonemize::plan(&b, &p, Phonemizer::KoCvvc);
    assert_eq!(units(&plan.units), [u("한", 0, 0), u("글", 1, 500)]);
}
