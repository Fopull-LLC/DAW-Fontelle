//! Korean lyrics: syllable blocks taken apart by Unicode's arithmetic,
//! romanised as Korean voicebanks name their sounds, with the sound changes
//! a sung line needs.

use fontelle_lang::hangul::{Syllable, compose, decompose, has_hangul, is_syllable, read, read_run};

fn romans(v: &Option<Vec<Syllable>>) -> Vec<String> {
    v.as_ref().unwrap().iter().map(Syllable::roman).collect()
}

#[test]
fn blocks_take_apart_and_back() {
    assert!(is_syllable('가'));
    assert!(is_syllable('힣'));
    assert!(!is_syllable('ㄱ'), "a lone jamo is not a block");
    assert!(!is_syllable('か'));
    assert!(has_hangul("x한"));
    assert_eq!(decompose('가'), Some((0, 0, 0)));
    assert_eq!(decompose('한'), Some((18, 0, 4)));
    assert_eq!(compose(18, 0, 4), Some('한'));
    assert_eq!(compose(19, 0, 0), None);
    for c in ['가', '깍', '뷁', '힣'] {
        let (i, v, f) = decompose(c).unwrap();
        assert_eq!(compose(i, v, f), Some(c));
    }
}

#[test]
fn syllables_romanise_onset_vowel_and_coda() {
    let han = &read("한").unwrap()[0];
    assert_eq!((han.onset, han.vowel, han.coda), ("h", "a", "n"));
    assert_eq!(han.open(), '하');
    assert_eq!(romans(&read("한글")), ["han", "geul"]);
    assert_eq!(romans(&read("사랑해")), ["sa", "rang", "hae"]);
    assert_eq!(romans(&read("뭐")), ["mwo"]);
    assert_eq!(romans(&read("의")), ["ui"]);
    // Codas are sung as one of seven sounds.
    assert_eq!(romans(&read("밖")), ["bak"]);
    assert_eq!(romans(&read("꽃")), ["kkot"]);
    assert_eq!(romans(&read("앞")), ["ap"]);
    assert_eq!(romans(&read("닭")), ["dak"]);
    assert_eq!(read("hello"), None);
    assert_eq!(read("한a"), None);
}

#[test]
fn a_coda_carries_into_a_following_vowel() {
    assert_eq!(romans(&read("음악")), ["eu", "mak"]);
    assert_eq!(romans(&read("밖에")), ["ba", "kke"]);
    assert_eq!(romans(&read("좋아")), ["jo", "a"], "ㅎ before a vowel is silent");
    assert_eq!(romans(&read("읽어")), ["il", "geo"], "a cluster leaves its first");
    // ㅇ is a sound of its own as a coda and stays.
    assert_eq!(romans(&read("사랑이")), ["sa", "rang", "i"]);
}

#[test]
fn stops_before_a_nasal_become_nasals() {
    assert_eq!(romans(&read("합니다")), ["ham", "ni", "da"]);
    assert_eq!(romans(&read("국물")), ["gung", "mul"]);
    assert_eq!(romans(&read("믿는")), ["min", "neun"]);
}

#[test]
fn a_line_sung_a_syllable_a_note_changes_across_notes() {
    let run = read_run(&["음", "악", "-", "가"]);
    assert_eq!(romans(&run[0]), ["eu"]);
    assert_eq!(romans(&run[1]), ["mak"]);
    assert_eq!(run[2], None);
    assert_eq!(romans(&run[3]), ["ga"], "a slur breaks the run");
}
