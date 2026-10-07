//! Chinese characters and Korean hangul in lyrics: read through
//! `fontelle-lang` into the syllables a bank's dictionary is keyed by
//! (pinyin for Chinese banks; romanised syllables or onset/vowel/coda
//! phonemes for Korean ones), words found across notes.

use fontelle_diffsinger::dict::Dictionary;
use fontelle_diffsinger::inventory::Inventory;
use fontelle_diffsinger::lyrics::words_of;
use fontelle_diffsinger::phonemize::{NotePhonemes, Ph, Reader, phonemize};
use fontelle_sing::PhraseNote;

fn notes(lyrics: &[&str]) -> Vec<PhraseNote> {
    lyrics
        .iter()
        .enumerate()
        .map(|(i, l)| PhraseNote::new(i as f64 * 0.5, 0.5, 60.0, *l))
        .collect()
}

fn syms(v: &[Ph]) -> Vec<&str> {
    v.iter().map(|p| p.symbol.as_str()).collect()
}

fn all(p: &NotePhonemes) -> Vec<&str> {
    syms(&p.lead).into_iter().chain(syms(&p.body)).collect()
}

/// An Opencpop-style Chinese dictionary: pinyin syllables to initials and
/// finals.
const ZH: &str = "symbols:
- {symbol: zh/n, type: nasal}
- {symbol: zh/h, type: fricative}
- {symbol: zh/w, type: semivowel}
- {symbol: zh/x, type: fricative}
- {symbol: zh/y, type: semivowel}
- {symbol: zh/q, type: fricative}
- {symbol: zh/i, type: vowel}
- {symbol: zh/ao, type: vowel}
- {symbol: zh/o, type: vowel}
- {symbol: zh/u, type: vowel}
- {symbol: zh/in, type: vowel}
- {symbol: zh/ing, type: vowel}
- {symbol: zh/ang, type: vowel}
- {symbol: zh/l, type: liquid}
- {symbol: zh/a, type: vowel}
entries:
- {grapheme: ni, phonemes: [zh/n, zh/i]}
- {grapheme: hao, phonemes: [zh/h, zh/ao]}
- {grapheme: wo, phonemes: [zh/w, zh/o]}
- {grapheme: qu, phonemes: [zh/q, zh/u]}
- {grapheme: yin, phonemes: [zh/y, zh/in]}
- {grapheme: xing, phonemes: [zh/x, zh/ing]}
- {grapheme: hang, phonemes: [zh/h, zh/ang]}
- {grapheme: la, phonemes: [zh/l, zh/a]}
";

fn zh() -> (Dictionary, Inventory) {
    let d = Dictionary::parse(ZH).unwrap();
    let inv = Inventory::parse(
        "SP\nAP\nzh/n\nzh/h\nzh/w\nzh/x\nzh/y\nzh/q\nzh/i\nzh/ao\nzh/o\nzh/u\nzh/in\nzh/ing\nzh/ang\nzh/l\nzh/a\n",
        false,
    )
    .unwrap();
    (d, inv)
}

#[test]
fn chinese_characters_read_as_pinyin_through_the_dictionary() {
    let (d, inv) = zh();
    let r = Reader {
        dict: &d,
        inventory: &inv,
        language: "zh",
    };
    assert_eq!(
        r.read("你好"),
        (
            vec!["zh/n".into(), "zh/i".into(), "zh/h".into(), "zh/ao".into()],
            false
        )
    );
    // A bracketed reading after a character overrides it.
    assert_eq!(r.read("行[hang]").0, ["zh/h", "zh/ang"]);
    // Pinyin typed as it is still reads.
    assert_eq!(r.read("hao").0, ["zh/h", "zh/ao"]);
}

#[test]
fn a_chinese_word_spreads_over_its_notes_and_words_are_found_across_notes() {
    let (d, inv) = zh();
    let r = Reader {
        dict: &d,
        inventory: &inv,
        language: "zh",
    };
    let n = notes(&["你好", "-"]);
    let p = phonemize(&n, &words_of(&n), &r);
    assert_eq!(syms(&p[0].lead), ["zh/n"]);
    assert_eq!(syms(&p[0].body), ["zh/i", "zh/h"]);
    assert_eq!(syms(&p[1].body), ["zh/ao"]);

    // 行 alone is xing; after 银 on the note before, it is the word 银行.
    let n = notes(&["我", "去", "银", "行"]);
    let p = phonemize(&n, &words_of(&n), &r);
    assert_eq!(all(&p[3]), ["zh/h", "zh/ang"]);
    let n = notes(&["行"]);
    let p = phonemize(&n, &words_of(&n), &r);
    assert_eq!(all(&p[0]), ["zh/x", "zh/ing"]);
    assert!(!p[0].guessed);
}

/// A Korean bank whose dictionary is keyed by romanised syllables.
const KO_SYLLABLES: &str = "symbols:
- {symbol: ko/h, type: fricative}
- {symbol: ko/a, type: vowel}
- {symbol: ko/n, type: nasal}
- {symbol: ko/g, type: stop}
- {symbol: ko/eu, type: vowel}
- {symbol: ko/l, type: liquid}
- {symbol: ko/m, type: nasal}
- {symbol: ko/k, type: stop}
entries:
- {grapheme: han, phonemes: [ko/h, ko/a, ko/n]}
- {grapheme: geul, phonemes: [ko/g, ko/eu, ko/l]}
- {grapheme: eu, phonemes: [ko/eu]}
- {grapheme: mak, phonemes: [ko/m, ko/a, ko/k]}
";

#[test]
fn hangul_reads_as_romanised_syllables_when_the_dictionary_has_them() {
    let d = Dictionary::parse(KO_SYLLABLES).unwrap();
    let inv = Inventory::parse(
        "SP\nAP\nko/h\nko/a\nko/n\nko/g\nko/eu\nko/l\nko/m\nko/k\n",
        false,
    )
    .unwrap();
    let r = Reader {
        dict: &d,
        inventory: &inv,
        language: "ko",
    };
    assert_eq!(
        r.read("한글").0,
        ["ko/h", "ko/a", "ko/n", "ko/g", "ko/eu", "ko/l"]
    );
    // 음악 a syllable a note: the ㅁ carries over to the second note.
    let n = notes(&["음", "악"]);
    let p = phonemize(&n, &words_of(&n), &r);
    assert_eq!(all(&p[0]), ["ko/eu"]);
    assert_eq!(all(&p[1]), ["ko/m", "ko/a", "ko/k"]);
    assert!(!p[1].guessed);
}

#[test]
fn hangul_reads_as_phonemes_when_the_dictionary_has_only_sounds() {
    // No entries: only the bank's symbols, as onset, vowel and coda.
    let d = Dictionary::parse(
        "symbols:
- {symbol: ko/s, type: fricative}
- {symbol: ko/a, type: vowel}
- {symbol: ko/r, type: liquid}
- {symbol: ko/ng, type: nasal}
- {symbol: ko/h, type: fricative}
- {symbol: ko/ae, type: vowel}
entries: []
",
    )
    .unwrap();
    let inv = Inventory::parse("SP\nAP\nko/s\nko/a\nko/r\nko/ng\nko/h\nko/ae\n", false).unwrap();
    let r = Reader {
        dict: &d,
        inventory: &inv,
        language: "ko",
    };
    let (p, guessed) = r.read("사랑해");
    assert_eq!(
        p,
        ["ko/s", "ko/a", "ko/r", "ko/a", "ko/ng", "ko/h", "ko/ae"]
    );
    assert!(!guessed);
}
