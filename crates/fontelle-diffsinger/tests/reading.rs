//! Lyrics to words, words to phonemes on notes.

use fontelle_diffsinger::dict::Dictionary;
use fontelle_diffsinger::inventory::Inventory;
use fontelle_diffsinger::lyrics::{Reading, Word, words_of};
use fontelle_diffsinger::phonemize::{NotePhonemes, Ph, Reader, edited, phonemize};
use fontelle_sing::{PhonemeEdit, PhraseNote};

fn notes(lyrics: &[&str]) -> Vec<PhraseNote> {
    lyrics
        .iter()
        .enumerate()
        .map(|(i, l)| PhraseNote::new(i as f64 * 0.5, 0.5, 60.0, *l))
        .collect()
}

fn text(s: &str) -> Reading {
    Reading::Text(s.into())
}

#[test]
fn each_lyric_is_a_word_of_its_own() {
    let w = words_of(&notes(&["hel", "lo", ""]));
    assert_eq!(
        w,
        vec![
            Word {
                notes: vec![0],
                reading: text("hel")
            },
            Word {
                notes: vec![1],
                reading: text("lo")
            },
            Word {
                notes: vec![2],
                reading: Reading::Default
            },
        ]
    );
}

#[test]
fn slurs_carry_the_word_on() {
    let w = words_of(&notes(&["amazing", "-", "-", "grace"]));
    assert_eq!(w.len(), 2);
    assert_eq!(w[0].notes, vec![0, 1, 2]);
    assert_eq!(w[0].reading, text("amazing"));
    assert_eq!(w[1].notes, vec![3]);
}

#[test]
fn a_slur_after_a_rest_sings_the_vowel_again() {
    let mut n = notes(&["la", "-"]);
    n[1].start = 0.8; // a rest after the first note
    let w = words_of(&n);
    assert_eq!(w.len(), 2);
    assert_eq!(w[1].reading, Reading::Resing);
    // ...and so does a phrase that starts with one.
    assert_eq!(words_of(&notes(&["-"]))[0].reading, Reading::Resing);
}

#[test]
fn hyphens_join_syllables_into_one_word() {
    let w = words_of(&notes(&["a-", "ma-", "zing", "grace"]));
    assert_eq!(w.len(), 2);
    assert_eq!(w[0].notes, vec![0, 1, 2]);
    assert_eq!(w[0].reading, text("amazing"));
    // A slur inside a joined word keeps the join going.
    let w = words_of(&notes(&["a-", "-", "zing"]));
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].notes, vec![0, 1, 2]);
    assert_eq!(w[0].reading, text("azing"));
    // A dangling hyphen on the last note is just dropped.
    assert_eq!(words_of(&notes(&["la-"]))[0].reading, text("la"));
}

#[test]
fn brackets_are_phonemes() {
    let w = words_of(&notes(&["[hh ax  l ow]"]));
    assert_eq!(
        w[0].reading,
        Reading::Phonemes(vec!["hh".into(), "ax".into(), "l".into(), "ow".into()])
    );
}

const DICT: &str = "symbols:
- symbol: en/aa
  type: vowel
- symbol: en/ax
  type: vowel
- symbol: en/ey
  type: vowel
- symbol: en/ih
  type: vowel
- symbol: en/ow
  type: vowel
- symbol: en/m
  type: nasal
- symbol: en/z
  type: fricative
- symbol: en/ng
  type: nasal
- symbol: en/hh
  type: fricative
- symbol: en/l
  type: liquid
- symbol: en/s
  type: fricative
- symbol: en/t
  type: stop
- symbol: en/k
  type: stop
- symbol: en/r
  type: liquid
entries:
- grapheme: amazing
  phonemes: [en/ax, en/m, en/ey, en/z, en/ih, en/ng]
- grapheme: hello
  phonemes: [en/hh, en/ax, en/l, en/ow]
- grapheme: star
  phonemes: [en/s, en/t, en/aa, en/r]
- grapheme: light
  phonemes: [en/l, en/aa, en/t]
- grapheme: la
  phonemes: [en/l, en/aa]
- grapheme: strike
  phonemes: [en/s, en/t, en/r, en/aa, en/k]
";

fn bank() -> (Dictionary, Inventory) {
    let d = Dictionary::parse(DICT).unwrap();
    let inv = Inventory::parse(
        "SP\nAP\nen/aa\nen/ax\nen/ey\nen/ih\nen/ow\nen/m\nen/z\nen/ng\nen/hh\nen/l\nen/s\nen/t\nen/k\nen/r\n",
        false,
    )
    .unwrap();
    (d, inv)
}

fn syms(v: &[Ph]) -> Vec<&str> {
    v.iter().map(|p| p.symbol.as_str()).collect()
}

#[test]
fn a_reader_resolves_symbols_with_the_language_prefix() {
    let (d, inv) = bank();
    let r = Reader {
        dict: &d,
        inventory: &inv,
        language: "en",
    };
    assert_eq!(r.resolve("en/aa").as_deref(), Some("en/aa"));
    assert_eq!(r.resolve("aa").as_deref(), Some("en/aa"));
    assert_eq!(r.resolve("SP").as_deref(), Some("SP"));
    assert_eq!(r.resolve("xx"), None);
    assert_eq!(
        r.read("hello"),
        (
            vec![
                "en/hh".into(),
                "en/ax".into(),
                "en/l".into(),
                "en/ow".into()
            ],
            false
        )
    );
    assert_eq!(r.read("Hello,").0.len(), 4, "case and punctuation");
    // Typed phonemes without brackets are a reading, not a guess.
    assert_eq!(
        r.read("hh aa"),
        (vec!["en/hh".into(), "en/aa".into()], false)
    );
    // A compound of known words is a guess built from them.
    let (p, guessed) = r.read("starlight");
    assert!(guessed);
    assert_eq!(
        p,
        ["en/s", "en/t", "en/aa", "en/r", "en/l", "en/aa", "en/t"]
    );
    // Nothing to go on: the default syllable, guessed.
    let (p, guessed) = r.read("qqq");
    assert!(guessed);
    assert_eq!(p, ["en/l", "en/aa"]);
    assert_eq!(r.default_syllable(), ["en/l", "en/aa"]);
}

fn read(lyrics: &[&str]) -> Vec<NotePhonemes> {
    let (d, inv) = bank();
    let r = Reader {
        dict: &d,
        inventory: &inv,
        language: "en",
    };
    let n = notes(lyrics);
    let w = words_of(&n);
    phonemize(&n, &w, &r)
}

#[test]
fn onsets_come_before_the_note_and_vowels_start_notes() {
    let p = read(&["hello"]);
    assert_eq!(syms(&p[0].lead), ["en/hh"]);
    // The rest of the word, two vowels and no second note: all on this one.
    assert_eq!(syms(&p[0].body), ["en/ax", "en/l", "en/ow"]);
    assert!(!p[0].guessed);
}

#[test]
fn a_word_spreads_a_syllable_a_note_over_its_slurs() {
    let p = read(&["amazing", "-", "-", "-"]);
    assert!(p[0].lead.is_empty());
    assert_eq!(syms(&p[0].body), ["en/ax", "en/m"]);
    assert_eq!(syms(&p[1].body), ["en/ey", "en/z"]);
    assert_eq!(syms(&p[2].body), ["en/ih", "en/ng"]);
    // More notes than syllables: the last vowel is held.
    assert!(p[3].is_empty());
}

#[test]
fn consonant_glide_vowel_starts_the_note_on_the_glide() {
    let p = read(&["strike"]);
    assert_eq!(syms(&p[0].lead), ["en/s", "en/t"]);
    assert_eq!(syms(&p[0].body), ["en/r", "en/aa", "en/k"]);
}

#[test]
fn a_resung_slur_takes_the_previous_vowel() {
    let (d, inv) = bank();
    let r = Reader {
        dict: &d,
        inventory: &inv,
        language: "en",
    };
    let mut n = notes(&["light", "-"]);
    n[1].start = 1.0;
    let p = phonemize(&n, &words_of(&n), &r);
    assert_eq!(syms(&p[1].body), ["en/aa"]);
    assert!(p[1].lead.is_empty());
}

#[test]
fn the_writers_phonemes_replace_the_reading_and_keep_their_timing() {
    let (d, inv) = bank();
    let r = Reader {
        dict: &d,
        inventory: &inv,
        language: "en",
    };
    let mut n = notes(&["hello", "la"]);
    n[1].phonemes = Some(vec![
        PhonemeEdit {
            symbol: "s".into(),
            offset: Some(-0.1),
        },
        PhonemeEdit {
            symbol: "en/ow".into(),
            offset: None,
        },
        PhonemeEdit {
            symbol: "nonsense".into(),
            offset: None,
        },
    ]);
    let p = phonemize(&n, &words_of(&n), &r);
    assert_eq!(syms(&p[0].lead), ["en/hh"], "the other note is untouched");
    assert_eq!(syms(&p[1].lead), ["en/s"]);
    assert_eq!(p[1].lead[0].offset, Some(-0.1));
    assert_eq!(syms(&p[1].body), ["en/ow"]);
    assert!(p[1].guessed, "a symbol the voice does not have was dropped");
    let e = edited(
        &[PhonemeEdit {
            symbol: "aa".into(),
            offset: Some(0.02),
        }],
        &r,
    );
    assert!(e.lead.is_empty());
    assert_eq!(
        e.body,
        vec![Ph {
            symbol: "en/aa".into(),
            offset: Some(0.02)
        }]
    );
}

#[test]
fn bracketed_phonemes_and_unknown_words() {
    let p = read(&["[hh ow]", "zzzz"]);
    assert_eq!(syms(&p[0].lead), ["en/hh"]);
    assert_eq!(syms(&p[0].body), ["en/ow"]);
    assert!(!p[0].guessed);
    assert!(p[1].guessed);
    assert_eq!(syms(&p[1].lead), ["en/l"]);
}

#[test]
fn kana_are_read_as_romaji_morae() {
    use fontelle_diffsinger::kana::morae;
    let m = |s: &str| morae(s).map(|v| v.join(" "));
    assert_eq!(m("か").as_deref(), Some("ka"));
    assert_eq!(m("きゃ").as_deref(), Some("kya"));
    assert_eq!(m("がっこう").as_deref(), Some("ga cl ko u"));
    assert_eq!(m("しゃべる").as_deref(), Some("sha be ru"));
    assert_eq!(m("ちぢつづ").as_deref(), Some("chi ji tsu zu"));
    assert_eq!(m("ん").as_deref(), Some("N"));
    assert_eq!(m("ファ").as_deref(), Some("fa"));
    assert_eq!(m("ヴ").as_deref(), Some("vu"));
    assert_eq!(m("ティ").as_deref(), Some("ti"));
    assert_eq!(m("を").as_deref(), Some("o"));
    assert_eq!(
        m("カー").as_deref(),
        Some("ka a"),
        "a long mark holds the vowel"
    );
    assert_eq!(m("ka"), None);
    assert_eq!(m("か1"), None);
    assert_eq!(m(""), None);
}

#[test]
fn a_japanese_reader_takes_kana() {
    let d = Dictionary::parse(
        "symbols:\n- symbol: ja/a\n  type: vowel\n- symbol: ja/k\n  type: stop\n- symbol: ja/N\n  type: vowel\n- symbol: ja/cl\n  type: stop\nentries:\n- grapheme: ka\n  phonemes: [ja/k, ja/a]\n- grapheme: n\n  phonemes: [ja/N]\n- grapheme: cl\n  phonemes: [ja/cl]\n",
    )
    .unwrap();
    let inv = Inventory::parse("SP\nAP\nja/a\nja/k\nja/N\nja/cl\n", false).unwrap();
    let r = Reader {
        dict: &d,
        inventory: &inv,
        language: "ja",
    };
    assert_eq!(r.read("か"), (vec!["ja/k".into(), "ja/a".into()], false));
    // ん is "N" in some dictionaries and "n" in others.
    assert_eq!(r.read("ん"), (vec!["ja/N".into()], false));
    assert_eq!(
        r.read("かっか"),
        (
            vec![
                "ja/k".into(),
                "ja/a".into(),
                "ja/cl".into(),
                "ja/k".into(),
                "ja/a".into()
            ],
            false
        )
    );
}
