//! Lyrics to aliases: each bank type's phonemizer, and the lyric rules
//! every singer shares (slurs, hyphen joins, bracketed phonemes).

use std::collections::BTreeMap;
use std::path::Path;

use fontelle_sing::{PhonemeEdit, Phrase, PhraseNote};
use fontelle_utau::bank::Voicebank;
use fontelle_utau::oto::OtoEntry;
use fontelle_utau::phonemize::{self, Phonemizer, PlannedUnit};

fn bank(aliases: &[&str]) -> Voicebank {
    bank_pre(aliases, 60.0)
}

fn bank_pre(aliases: &[&str], pre: f64) -> Voicebank {
    let entries = aliases
        .iter()
        .enumerate()
        .map(|(i, a)| OtoEntry {
            alias: a.to_string(),
            file: format!("{i}.wav").into(),
            folder: String::new(),
            offset: 0.0,
            consonant: pre + 40.0,
            cutoff: -500.0,
            preutterance: pre,
            overlap: 15.0,
        })
        .collect();
    Voicebank::from_parts(Path::new("/b"), "b", entries, vec![])
}

fn phrase(notes: &[(f64, f64, &str)]) -> Phrase {
    Phrase {
        voice: "v".into(),
        language: "ja".into(),
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

/// (alias, note, anchor, tail) of each unit, anchors to the millisecond.
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

const KANA: [&str; 10] = ["あ", "い", "う", "え", "お", "か", "き", "さ", "し", "ら"];

#[test]
fn the_bank_type_is_read_from_its_aliases() {
    assert_eq!(phonemize::detect(&bank(&KANA)), Phonemizer::JaCv);
    // Single-sound banks carry "- か" and "* か" starts: still CV.
    let mut cv: Vec<String> = KANA.iter().map(|k| k.to_string()).collect();
    for k in KANA.iter().chain(&KANA).chain(&KANA).chain(&KANA) {
        cv.push(format!("- {k}"));
        cv.push(format!("* {k}"));
    }
    let cv: Vec<&str> = cv.iter().map(String::as_str).collect();
    assert_eq!(phonemize::detect(&bank(&cv)), Phonemizer::JaCv);
    let mut vcv: Vec<String> = KANA.iter().map(|k| format!("- {k}")).collect();
    for v in ["a", "i", "u", "e", "o"] {
        for k in KANA {
            vcv.push(format!("{v} {k}"));
        }
    }
    let vcv: Vec<&str> = vcv.iter().map(String::as_str).collect();
    assert_eq!(phonemize::detect(&bank(&vcv)), Phonemizer::JaVcv);
    let mut cvvc: Vec<String> = KANA.iter().map(|k| k.to_string()).collect();
    for v in ["a", "i", "u", "e", "o"] {
        for c in ["k", "s", "r", "t", "n", "m"] {
            cvvc.push(format!("{v} {c}"));
        }
    }
    let cvvc: Vec<&str> = cvvc.iter().map(String::as_str).collect();
    assert_eq!(phonemize::detect(&bank(&cvvc)), Phonemizer::JaCvvc);
    let mut arpa = Vec::new();
    for a in ["hh", "l", "ah", "ow", "aa", "iy", "k", "s", "t"] {
        arpa.push(format!("- {a}"));
        arpa.push(format!("{a} -"));
        for b in ["ah", "ow", "l", "eh", "aa", "iy"] {
            arpa.push(format!("{a} {b}"));
        }
    }
    let arpa: Vec<&str> = arpa.iter().map(String::as_str).collect();
    assert_eq!(phonemize::detect(&bank(&arpa)), Phonemizer::EnArpasing);
    let vccv = [
        "-ka", "ka", "-re", "re", "Ed-", "a-", "@ k", "ak", "kt", "8a", "E d", "-rE", "rE", "@l",
        "Ek", "st", "-sE", "sE", "i-", "o-", "u-", "bd", "-b@", "b@",
    ];
    assert_eq!(phonemize::detect(&bank(&vccv)), Phonemizer::EnVccv);
}

#[test]
fn cv_sings_one_alias_per_mora_and_slurs_hold() {
    let b = bank(&KANA);
    let p = phrase(&[
        (0.0, 0.5, "か"),
        (0.5, 0.5, "-"),
        (1.0, 0.5, "sa"),
        (1.5, 0.5, "シ"),
    ]);
    let plan = phonemize::plan(&b, &p, Phonemizer::JaCv);
    assert_eq!(
        units(&plan.units),
        [u("か", 0, 0), u("さ", 2, 1000), u("し", 3, 1500)]
    );
    assert_eq!(plan.guessed, [false; 4]);
}

#[test]
fn cv_prefers_a_starting_alias_after_a_rest() {
    let mut aliases = KANA.to_vec();
    aliases.push("- か");
    let b = bank(&aliases);
    let p = phrase(&[(0.0, 0.5, "か"), (0.5, 0.5, "か"), (1.5, 0.5, "か")]);
    let plan = phonemize::plan(&b, &p, Phonemizer::JaCv);
    assert_eq!(
        units(&plan.units),
        [u("- か", 0, 0), u("か", 1, 500), u("- か", 2, 1500)]
    );
}

#[test]
fn several_morae_on_one_note_spread_over_its_slurs_or_split_it() {
    let b = bank(&KANA);
    let p = phrase(&[(0.0, 0.5, "かさ"), (0.5, 0.5, "-"), (1.0, 0.4, "しら")]);
    let plan = phonemize::plan(&b, &p, Phonemizer::JaCv);
    assert_eq!(
        units(&plan.units),
        [
            u("か", 0, 0),
            u("さ", 1, 500),
            u("し", 2, 1000),
            u("ら", 2, 1200)
        ]
    );
}

#[test]
fn bracketed_lyrics_are_aliases_as_written() {
    let b = bank(&["a か", "breath", "あ"]);
    let p = phrase(&[(0.0, 0.5, "[breath]"), (0.5, 0.5, "[a か]")]);
    let plan = phonemize::plan(&b, &p, Phonemizer::JaCv);
    assert_eq!(units(&plan.units), [u("breath", 0, 0), u("a か", 1, 500)]);
}

#[test]
fn hand_phonemes_replace_the_reading_and_keep_their_timing() {
    let b = bank(&KANA);
    let mut p = phrase(&[(0.0, 0.5, "か"), (0.5, 0.5, "さ")]);
    p.notes[1].phonemes = Some(vec![
        PhonemeEdit {
            symbol: "し".into(),
            offset: Some(-0.1),
        },
        PhonemeEdit {
            symbol: "ら".into(),
            offset: None,
        },
    ]);
    let plan = phonemize::plan(&b, &p, Phonemizer::JaCv);
    let got = units(&plan.units);
    assert_eq!(got[0], u("か", 0, 0));
    assert_eq!(got[1].0, "し");
    assert_eq!(plan.units[1].start, Some(0.4));
    assert_eq!(got[2].0, "ら");
    assert!(got[2].2 > 500, "an untimed phoneme follows inside the note");
}

#[test]
fn an_unknown_lyric_is_guessed() {
    let b = bank(&KANA);
    let p = phrase(&[(0.0, 0.5, "zzz")]);
    let plan = phonemize::plan(&b, &p, Phonemizer::JaCv);
    assert_eq!(plan.guessed, [true]);
    assert_eq!(plan.units.len(), 1);
}

#[test]
fn vcv_joins_from_the_vowel_before_and_ends_on_r() {
    let mut aliases = vec!["- か".to_string(), "- さ".to_string(), "a R".to_string()];
    for k in KANA {
        aliases.push(format!("a {k}"));
    }
    let aliases: Vec<&str> = aliases.iter().map(String::as_str).collect();
    let b = bank(&aliases);
    let p = phrase(&[
        (0.0, 0.5, "ka"),
        (0.5, 0.5, "さ"),
        (1.0, 0.5, "-"),
        (2.0, 0.5, "さ"),
    ]);
    let plan = phonemize::plan(&b, &p, Phonemizer::JaVcv);
    assert_eq!(
        units(&plan.units),
        [
            u("- か", 0, 0),
            u("a さ", 1, 500),
            tail("a R", 2, 1500),
            u("- さ", 3, 2000),
            tail("a R", 3, 2500),
        ]
    );
}

#[test]
fn cvvc_adds_the_vowel_to_consonant_before_the_next_note() {
    let b = bank_pre(
        &["- か", "か", "さ", "あ", "a s", "a k", "a R", "a あ"],
        120.0,
    );
    let p = phrase(&[
        (0.0, 0.5, "か"),
        (0.5, 0.5, "さ"),
        (1.0, 0.5, "あ"),
        (1.5, 0.2, "か"),
    ]);
    let plan = phonemize::plan(&b, &p, Phonemizer::JaCvvc);
    assert_eq!(
        units(&plan.units),
        [
            u("- か", 0, 0),
            // The next CV's preutterance before it.
            u("a s", 0, 380),
            u("さ", 1, 500),
            u("a あ", 2, 1000),
            u("a k", 2, 1380),
            u("か", 3, 1500),
            tail("a R", 3, 1700),
        ]
    );
}

#[test]
fn cvvc_vowel_to_consonant_takes_at_most_half_the_note() {
    let b = bank_pre(&["か", "さ", "a s"], 300.0);
    let p = phrase(&[(0.0, 0.2, "か"), (0.2, 0.5, "さ")]);
    let plan = phonemize::plan(&b, &p, Phonemizer::JaCvvc);
    assert_eq!(units(&plan.units)[1], u("a s", 0, 100));
}

fn arpa_bank(extra: &[&str]) -> Voicebank {
    let mut a = vec![
        "- hh", "hh ah", "ah l", "l ow", "ow -", "- r", "r iy", "iy d", "d -",
    ];
    a.extend_from_slice(extra);
    bank(&a)
}

#[test]
fn arpasing_sings_diphones_with_consonants_before_the_beat() {
    let b = arpa_bank(&[]);
    let mut p = phrase(&[(0.0, 1.0, "hello")]);
    p.language = "en".into();
    let plan = phonemize::plan(&b, &p, Phonemizer::EnArpasing);
    assert_eq!(
        units(&plan.units),
        [
            u("- hh", 0, -60),
            u("hh ah", 0, 0),
            u("ah l", 0, 440),
            u("l ow", 0, 500),
            tail("ow -", 0, 1000),
        ]
    );
    assert_eq!(plan.guessed, [false]);
}

#[test]
fn a_word_spreads_over_slurs_and_hyphen_joins() {
    let b = arpa_bank(&[]);
    for lyrics in [["hello", "-"], ["hel-", "lo"]] {
        let mut p = phrase(&[(0.0, 0.5, lyrics[0]), (0.5, 0.5, lyrics[1])]);
        p.language = "en".into();
        let plan = phonemize::plan(&b, &p, Phonemizer::EnArpasing);
        assert_eq!(
            units(&plan.units),
            [
                u("- hh", 0, -60),
                u("hh ah", 0, 0),
                u("ah l", 1, 440),
                u("l ow", 1, 500),
                tail("ow -", 1, 1000),
            ],
            "{lyrics:?}"
        );
    }
}

#[test]
fn arpasing_falls_back_to_a_starting_diphone() {
    let b = bank(&["- hh", "hh ah", "- l", "l ow", "ow -"]);
    let mut p = phrase(&[(0.0, 1.0, "hello")]);
    p.language = "en".into();
    let plan = phonemize::plan(&b, &p, Phonemizer::EnArpasing);
    let aliases: Vec<String> = plan.units.iter().map(|u| u.alias.clone()).collect();
    assert_eq!(aliases, ["- hh", "hh ah", "- l", "l ow", "ow -"]);
}

#[test]
fn arpasing_takes_bracketed_phonemes() {
    let b = arpa_bank(&[]);
    let mut p = phrase(&[(0.0, 1.0, "[r iy d]")]);
    p.language = "en".into();
    let plan = phonemize::plan(&b, &p, Phonemizer::EnArpasing);
    let aliases: Vec<String> = plan.units.iter().map(|u| u.alias.clone()).collect();
    assert_eq!(aliases, ["- r", "r iy", "iy d", "d -"]);
}

#[test]
fn vccv_sings_read_as_its_bank_names_it() {
    let b = bank(&["-rE", "rE", "Ed-", "E d", "d-", "E-", "-re", "ed-"]);
    // CMUdict reads "read" as the past tense, r eh d.
    let mut p = phrase(&[(0.0, 1.0, "read")]);
    p.language = "en".into();
    let plan = phonemize::plan(&b, &p, Phonemizer::EnVccv);
    assert_eq!(units(&plan.units), [u("-re", 0, 0), tail("ed-", 0, 940)]);
    let mut p = phrase(&[(0.0, 1.0, "[r iy d]")]);
    p.language = "en".into();
    let plan = phonemize::plan(&b, &p, Phonemizer::EnVccv);
    assert_eq!(units(&plan.units), [u("-rE", 0, 0), tail("Ed-", 0, 940)]);
}

#[test]
fn english_cvvc_sings_x_sampa_syllables() {
    let mut x: Vec<String> = Vec::new();
    for v in ["@", "{", "eI", "aI", "oU", "3", "A", "E", "i", "u"] {
        for c in ["b", "d", "tS", "dZ", "h", "l", "r"] {
            x.push(format!("{c}{v}"));
            x.push(format!("- {c}{v}"));
            x.push(format!("{v} {c}"));
            x.push(format!("{v} {c}-"));
        }
        x.push(format!("{v} -"));
    }
    let x: Vec<&str> = x.iter().map(String::as_str).collect();
    let b = bank(&x);
    assert_eq!(phonemize::detect(&b), Phonemizer::EnCvvc);
    let mut p = phrase(&[(0.0, 1.0, "hello")]);
    p.language = "en".into();
    let plan = phonemize::plan(&b, &p, Phonemizer::EnCvvc);
    // hh ah l ow: ah has no V in this bank, so its fallback @.
    assert_eq!(
        units(&plan.units),
        [
            u("- h@", 0, 0),
            u("@ l", 0, 440),
            u("loU", 0, 500),
            tail("oU -", 0, 1000)
        ]
    );
    let mut p = phrase(&[(0.0, 1.0, "[r iy d]")]);
    p.language = "en".into();
    let plan = phonemize::plan(&b, &p, Phonemizer::EnCvvc);
    assert_eq!(units(&plan.units), [u("- ri", 0, 0), tail("i d-", 0, 940)]);
}

/// Bracketed phonemes a bank does not have as aliases: `[s a]` on Kasane
/// Teto's Japanese bank was a note sung as nothing, with nothing said. It
/// is read as the lyric its sounds spell ("sa": さ), and the note is
/// marked as guessed so the roll flags it.
#[test]
fn bracketed_sounds_the_bank_lacks_are_read_as_a_lyric_and_flagged() {
    let b = bank(&KANA);
    let p = phrase(&[(0.0, 0.5, "[s a]")]);
    let plan = phonemize::plan(&b, &p, Phonemizer::JaCv);
    let aliases: Vec<&str> = plan.units.iter().map(|u| u.alias.as_str()).collect();
    assert_eq!(aliases, ["さ"]);
    assert_eq!(plan.guessed, [true]);
    // Aliases it has are sung as written, not guessed.
    let p = phrase(&[(0.0, 0.5, "[か]")]);
    let plan = phonemize::plan(&b, &p, Phonemizer::JaCv);
    let aliases: Vec<&str> = plan.units.iter().map(|u| u.alias.as_str()).collect();
    assert_eq!(aliases, ["か"]);
    assert_eq!(plan.guessed, [false]);
}
