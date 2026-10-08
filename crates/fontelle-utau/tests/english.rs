//! English on UTAU banks: what common lines are sung as.
//!
//! Ty: "the english wording sounds off when i try to force it to speak
//! english". On Kasane Teto's English bank (X-SAMPA CVVC) "world" was sung
//! `oU w`, `w3`, `3 l` with no `d`, "hand" lost its `n d`, clusters such as
//! `st`, `tw`, `bju` were never used and every unstressed "uh" was the
//! stressed `V`. The fixture below has the shape of that bank's aliases.

use std::collections::BTreeMap;
use std::path::Path;

use fontelle_sing::{Phrase, PhraseNote};
use fontelle_utau::bank::Voicebank;
use fontelle_utau::dict;
use fontelle_utau::oto::OtoEntry;
use fontelle_utau::phonemize::{self, Phonemizer};

fn bank(aliases: &[String]) -> Voicebank {
    let entries = aliases
        .iter()
        .enumerate()
        .map(|(i, a)| OtoEntry {
            alias: a.clone(),
            file: format!("{i}.wav").into(),
            folder: String::new(),
            offset: 0.0,
            consonant: 100.0,
            cutoff: -500.0,
            preutterance: 60.0,
            overlap: 15.0,
        })
        .collect();
    Voicebank::from_parts(Path::new("/b"), "b", entries, vec![])
}

const VOWELS: [&str; 16] = [
    "@", "{", "3", "A", "aI", "aU", "E", "eI", "i", "I", "oU", "O", "OI", "u", "U", "V",
];
const CONSONANTS: [&str; 23] = [
    "b", "d", "D", "dZ", "f", "g", "j", "k", "l", "m", "n", "N", "p", "r", "s", "S", "t", "T",
    "tS", "v", "w", "z", "Z",
];

/// The shape of Kasane Teto's English bank: CV ("- kA", "kA"; no "bV"),
/// VC ("A k", "A k-"), VV, "h" only before a vowel ("hA", "@ hA"),
/// released consonants alone ("t"), joined onsets ("- st", "str", "tw"),
/// "ju" clusters ("bju"), "r" before a consonant ("r g"), and endings
/// over a final cluster ("l d-", "s kt-").
fn teto_like() -> Voicebank {
    let mut a: Vec<String> = Vec::new();
    for v in VOWELS {
        a.push(format!("- {v}"));
        a.push(v.to_string());
        a.push(format!("{v} -"));
        for w in VOWELS {
            a.push(format!("{v} {w}"));
            a.push(format!("{v} h{w}"));
        }
        a.push(format!("h{v}"));
        a.push(format!("- h{v}"));
        for c in CONSONANTS {
            if !(c == "b" && v == "V") {
                a.push(format!("{c}{v}"));
                a.push(format!("- {c}{v}"));
            }
            a.push(format!("{v} {c}"));
            a.push(format!("{v} {c}-"));
        }
    }
    for c in [
        "b", "d", "g", "k", "p", "t", "s", "z", "f", "v", "S", "T", "D", "n", "m", "l", "tS", "dZ",
    ] {
        a.push(c.to_string());
    }
    for cc in [
        "st", "str", "sk", "sp", "spl", "tw", "pl", "pr", "bl", "br", "kr", "kl", "tr", "dr", "sw",
        "sl", "sm", "sn", "kw", "gr", "fl", "fr",
    ] {
        a.push(cc.to_string());
        a.push(format!("- {cc}"));
    }
    for c in ["b", "k", "p", "f", "m"] {
        a.push(format!("{c}ju"));
        a.push(format!("- {c}ju"));
    }
    for c in ["b", "d", "g", "k", "t", "m", "n", "s", "l"] {
        a.push(format!("r {c}"));
    }
    for e in [
        "l d-", "n d-", "n t-", "s kt-", "l z-", "k s-", "k st-", "n z-", "s t-", "n dz-", "m z-",
        "l t-",
    ] {
        a.push(e.to_string());
    }
    bank(&a)
}

/// One note per word, touching, 0.5 s each; the line ends in a rest.
fn line(words: &str) -> Phrase {
    Phrase {
        voice: "v".into(),
        language: "en".into(),
        notes: words
            .split_whitespace()
            .enumerate()
            .map(|(i, w)| PhraseNote::new(i as f64 * 0.5, 0.5, 60.0, w))
            .collect(),
        curves: BTreeMap::new(),
        extra_curves: BTreeMap::new(),
        settings: BTreeMap::new(),
        state: None,
        start: 0.0,
        note_ids: Vec::new(),
    }
}

fn sung(b: &Voicebank, words: &str, kind: Phonemizer) -> Vec<String> {
    phonemize::plan(b, &line(words), kind)
        .units
        .iter()
        .map(|u| u.alias.clone())
        .collect()
}

#[test]
fn unstressed_uh_is_a_schwa() {
    let d = dict::cmudict();
    assert_eq!(d.lookup("hello").unwrap(), ["hh", "ax", "l", "ow"]);
    assert_eq!(d.lookup("the").unwrap(), ["dh", "ax"]);
    assert_eq!(d.lookup("a").unwrap(), ["ax"]);
    // Stressed, it stays "ah".
    assert_eq!(d.lookup("love").unwrap(), ["l", "ah", "v"]);
    assert!(dict::is_vowel("ax"));
}

#[test]
fn final_clusters_are_sung_to_the_end() {
    let b = teto_like();
    let k = Phonemizer::EnCvvc;
    assert_eq!(
        sung(&b, "hello world", k),
        ["- h@", "@ l", "loU", "oU w", "w3", "3 l", "l d-"]
    );
    assert_eq!(sung(&b, "asked", k), ["- {", "{ s", "s kt-"]);
    assert_eq!(sung(&b, "next", k), ["- nE", "E k", "k st-"]);
    assert_eq!(
        sung(&b, "she sells", k),
        ["- Si", "i s", "sE", "E l", "l z-"]
    );
    assert_eq!(sung(&b, "I'm", k), ["- aI", "aI m-"]);
}

#[test]
fn a_line_is_sung_with_its_clusters_and_releases() {
    let b = teto_like();
    let k = Phonemizer::EnCvvc;
    // "want to": one t; "hold your": d released before "jO"; "to hold":
    // the bank's "u hoU".
    assert_eq!(
        sung(&b, "I want to hold your hand", k),
        [
            "- aI", "aI w", "wA", "A n", "tu", "u hoU", "oU l", "d", "jO", "O r", "h{", "{ n",
            "n d-"
        ]
    );
    assert_eq!(
        sung(&b, "twinkle twinkle little star", k),
        [
            "- tw", "wI", "I N", "k@", "@ l", "tw", "wI", "I N", "k@", "@ l", "lI", "I t", "t@",
            "@ l", "st", "tA", "A r-"
        ]
    );
    assert_eq!(
        sung(&b, "it's a beautiful day", k),
        [
            "- I", "I t", "t", "s@", "@ b", "bju", "u t", "t@", "@ f", "f@", "@ l", "deI", "eI -"
        ]
    );
    assert_eq!(
        sung(&b, "don't stop", k),
        ["- doU", "oU n", "t", "st", "tA", "A p-"]
    );
    assert_eq!(sung(&b, "street", k), ["- str", "ri", "i t-"]);
    assert_eq!(
        sung(&b, "we're gonna", k),
        ["- wi", "i r", "r g", "gA", "A n", "n@", "@ -"]
    );
}

#[test]
fn units_keep_time_order_and_land_before_the_beat() {
    let b = teto_like();
    let plan = phonemize::plan(&b, &line("don't stop"), Phonemizer::EnCvvc);
    let anchors: Vec<f64> = plan.units.iter().map(|u| u.anchor).collect();
    assert!(anchors.windows(2).all(|w| w[0] < w[1]), "{anchors:?}");
    // "tA" is on the second note's beat; its consonants before it.
    let ta = plan.units.iter().find(|u| u.alias == "tA").unwrap();
    assert!((ta.anchor - 0.5).abs() < 1e-9);
    let st = plan.units.iter().find(|u| u.alias == "st").unwrap();
    assert!(st.anchor < 0.5 && st.anchor > 0.3);
    let end = plan.units.last().unwrap();
    assert!(end.tail && end.anchor <= 1.0);
}

#[test]
fn arpasing_sings_a_schwa_as_its_banks_names_it() {
    // A bank with "ax" uses it; one without falls back to "ah" on both
    // sides of the diphone.
    let with: Vec<String> = [
        "- hh", "hh ax", "hh ah", "ax l", "ah l", "l ow", "ow -", "- dh", "dh ax", "ax -",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let b = bank(&with);
    assert_eq!(
        sung(&b, "hello", Phonemizer::EnArpasing),
        ["- hh", "hh ax", "ax l", "l ow", "ow -"]
    );
    let without: Vec<String> = [
        "- hh", "hh ah", "ah l", "l ow", "ow -", "- dh", "dh ah", "ah -",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let b = bank(&without);
    assert_eq!(
        sung(&b, "hello", Phonemizer::EnArpasing),
        ["- hh", "hh ah", "ah l", "l ow", "ow -"]
    );
    assert_eq!(
        sung(&b, "the", Phonemizer::EnArpasing),
        ["- dh", "dh ah", "ah -"]
    );
}

#[test]
fn vccv_sings_a_schwa_as_uh() {
    let a: Vec<String> = ["-hu", "hu", "ul", "lO", "O-", "u l"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let b = bank(&a);
    let s = sung(&b, "hello", Phonemizer::EnVccv);
    assert_eq!(s.first().map(String::as_str), Some("-hu"), "{s:?}");
}
