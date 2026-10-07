//! Chinese lyrics: a character's most common reading, toneless; words that
//! read a character otherwise, even sung a character a note; a bracketed
//! reading after a character for anything else.

use fontelle_lang::pinyin::{has_hanzi, is_hanzi, is_syllable, read, read_run, reading, split};

fn s(v: &[&str]) -> Option<Vec<String>> {
    Some(v.iter().map(|x| x.to_string()).collect())
}

#[test]
fn characters_are_told_from_kana_hangul_and_latin() {
    assert!(is_hanzi('你'));
    assert!(is_hanzi('㐀'), "extension A");
    assert!(is_hanzi('𠀀'), "extension B");
    assert!(!is_hanzi('か'));
    assert!(!is_hanzi('한'));
    assert!(!is_hanzi('a'));
    assert!(has_hanzi("la你"));
    assert!(!has_hanzi("hello"));
}

#[test]
fn each_character_reads_its_most_common_reading_without_tones() {
    assert_eq!(reading('你'), Some("ni"));
    assert_eq!(reading('好'), Some("hao"));
    assert_eq!(reading('爱'), Some("ai"));
    assert_eq!(reading('中'), Some("zhong"));
    // ü after l and n is v; after j, q, x, y it is u.
    assert_eq!(reading('绿'), Some("lv"));
    assert_eq!(reading('女'), Some("nv"));
    assert_eq!(reading('去'), Some("qu"));
    assert_eq!(reading('雪'), Some("xue"));
    assert_eq!(reading('的'), Some("de"));
    // In songs 长 is far more often cháng (long) than zhǎng (grow).
    assert_eq!(reading('长'), Some("chang"));
    assert_eq!(reading('a'), None);
}

#[test]
fn a_lyric_is_a_syllable_per_character() {
    assert_eq!(read("你好"), s(&["ni", "hao"]));
    assert_eq!(read("我爱你。"), s(&["wo", "ai", "ni"]), "punctuation is not sung");
    assert_eq!(read("hello"), None);
    assert_eq!(read("你ok"), None, "mixed scripts are not guessed at");
    assert_eq!(read(""), None);
}

#[test]
fn common_words_read_their_characters_as_the_word_does() {
    assert_eq!(read("行"), s(&["xing"]));
    assert_eq!(read("银行"), s(&["yin", "hang"]));
    assert_eq!(read("长大"), s(&["zhang", "da"]));
    assert_eq!(read("还是"), s(&["hai", "shi"]));
    assert_eq!(read("音乐"), s(&["yin", "yue"]));
    assert_eq!(read("快乐"), s(&["kuai", "le"]));
    assert_eq!(read("睡觉"), s(&["shui", "jiao"]));
    assert_eq!(read("重新"), s(&["chong", "xin"]));
}

#[test]
fn a_word_sung_a_character_a_note_is_still_the_word() {
    assert_eq!(
        read_run(&["我", "去", "银", "行"]),
        vec![s(&["wo"]), s(&["qu"]), s(&["yin"]), s(&["hang"])]
    );
    // A lyric that is not Chinese breaks the run but is not read.
    assert_eq!(
        read_run(&["音", "-", "乐"]),
        vec![s(&["yin"]), None, s(&["le"])]
    );
}

#[test]
fn a_bracketed_reading_after_a_character_overrides_it() {
    assert_eq!(read("行[hang]"), s(&["hang"]));
    // Tones and the ü spelling are accepted and normalised.
    assert_eq!(read("行[hang2]"), s(&["hang"]));
    assert_eq!(read("绿[lü4]"), s(&["lv"]));
    assert_eq!(read("银行[xing]"), s(&["yin", "xing"]), "the override wins over the word");
    assert_eq!(read_run(&["长[zhang]", "大"]), vec![s(&["zhang"]), s(&["da"])]);
}

#[test]
fn syllables_split_into_initial_and_final() {
    assert_eq!(split("zhuang"), ("zh", "uang"));
    assert_eq!(split("ai"), ("", "ai"));
    assert_eq!(split("ni"), ("n", "i"));
    assert_eq!(split("yu"), ("y", "u"));
    assert_eq!(split("chi"), ("ch", "i"));
    assert!(is_syllable("zhuang"));
    assert!(is_syllable("lv"));
    assert!(is_syllable("a"));
    assert!(is_syllable("ka"), "咔");
    assert!(!is_syllable("kya"));
    assert!(!is_syllable("tsu"));
}
