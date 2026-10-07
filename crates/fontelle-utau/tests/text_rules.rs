//! Kana, romaji and English pronunciation.

use fontelle_utau::{dict, kana};

#[test]
fn katakana_and_romaji_become_hiragana() {
    assert_eq!(kana::to_hiragana("カ"), "か");
    assert_eq!(kana::to_hiragana("ka"), "か");
    assert_eq!(kana::to_hiragana("KYA"), "きゃ");
    assert_eq!(kana::to_hiragana("shi"), "し");
    assert_eq!(kana::to_hiragana("tsu"), "つ");
    assert_eq!(kana::to_hiragana("n"), "ん");
    assert_eq!(kana::to_hiragana("sakura"), "さくら");
    assert_eq!(kana::to_hiragana("ヴァ"), "ゔぁ");
    assert_eq!(kana::to_hiragana("さ"), "さ");
    // Not Japanese: left alone.
    assert_eq!(kana::to_hiragana("hello"), "hello");
    assert_eq!(kana::to_hiragana("breath"), "breath");
}

#[test]
fn kana_romanise_and_split() {
    assert_eq!(kana::kana_to_romaji("か").as_deref(), Some("ka"));
    assert_eq!(kana::kana_to_romaji("きゃ").as_deref(), Some("kya"));
    assert_eq!(kana::kana_to_romaji("ん").as_deref(), Some("n"));
    assert_eq!(kana::kana_to_romaji("し").as_deref(), Some("shi"));
    assert_eq!(kana::kana_to_romaji("x"), None);
    assert_eq!(
        kana::split_romaji("kya"),
        ("ky".to_string(), "a".to_string())
    );
    assert_eq!(kana::split_romaji("a"), (String::new(), "a".to_string()));
    assert_eq!(kana::split_romaji("n"), (String::new(), "n".to_string()));
    assert_eq!(
        kana::split_romaji("shi"),
        ("sh".to_string(), "i".to_string())
    );
    assert_eq!(kana::vowel_of("か"), Some("a"));
    assert_eq!(kana::vowel_of("きゃ"), Some("a"));
    assert_eq!(kana::vowel_of("ん"), Some("n"));
    assert_eq!(kana::vowel_of("ka"), Some("a"));
    assert_eq!(kana::morae("さくら"), vec!["さ", "く", "ら"]);
    assert_eq!(kana::morae("きゃりー"), vec!["きゃ", "り", "ー"]);
}

#[test]
fn the_dictionary_knows_english_words() {
    let d = dict::cmudict();
    assert!(d.len() > 100_000);
    assert_eq!(d.lookup("hello").unwrap(), ["hh", "ah", "l", "ow"]);
    assert_eq!(
        d.lookup("Hello").unwrap(),
        ["hh", "ah", "l", "ow"],
        "any case"
    );
    assert_eq!(d.lookup("world's").unwrap(), ["w", "er", "l", "d", "z"]);
    assert!(d.lookup("qzxqzx").is_none());
}

#[test]
fn unknown_words_are_guessed_with_vowels() {
    let g = dict::guess("blorpt");
    assert!(g.iter().any(|p| dict::is_vowel(p)), "{g:?}");
    assert_eq!(g.first().map(String::as_str), Some("b"));
    assert!(!dict::guess("zank").is_empty());
}

#[test]
fn syllables_take_the_consonants_english_allows() {
    let s = |p: &str| -> Vec<Vec<String>> {
        dict::syllabify(&p.split(' ').map(String::from).collect::<Vec<_>>())
    };
    assert_eq!(s("hh ah l ow"), [vec!["hh", "ah"], vec!["l", "ow"]]);
    assert_eq!(
        s("ah m ey z ih ng"),
        [vec!["ah"], vec!["m", "ey"], vec!["z", "ih", "ng"]]
    );
    // "extra": k goes to the first syllable, s t r start the second.
    assert_eq!(
        s("eh k s t r ah"),
        [vec!["eh", "k"], vec!["s", "t", "r", "ah"]]
    );
    assert_eq!(s("s t r ao ng"), [vec!["s", "t", "r", "ao", "ng"]]);
    assert_eq!(s("hh m"), [vec!["hh", "m"]], "no vowel: one syllable");
}

/// The embedded dictionary is reviewed data: CMUdict (BSD-2-Clause, its
/// notice in `data/CMUDICT-LICENSE`), cmusphinx/cmudict at commit
/// 74790861f652b15e4ac49015a90074ad62a27690, first pronunciations, stress
/// removed. A changed file fails here until it is reviewed again.
#[test]
fn dictionary_file_hash_is_pinned() {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/data/cmudict.tsv.gz")).unwrap();
    let hex: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        hex,
        "1e7a1ebb4f55ab1990447eff4c8fa407e41b4c9a362d90344142d766459a3168"
    );
    let notice =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/data/CMUDICT-LICENSE"))
            .unwrap();
    assert!(notice.contains("Carnegie Mellon University"));
}
