//! Kana and romaji.
//!
//! Lyrics reach a Japanese bank as hiragana, katakana or romaji; banks name
//! their samples in hiragana (most) or romaji (some). Everything is brought
//! to hiragana first, and romanised where an alias needs a vowel or a
//! consonant on its own (`a か`, `a k`).

use std::collections::HashMap;
use std::sync::OnceLock;

/// (hiragana, romaji); the first romaji listed for a kana is the one it
/// romanises to, the others are also read.
const TABLE: &[(&str, &str)] = &[
    ("きゃ", "kya"),
    ("きゅ", "kyu"),
    ("きょ", "kyo"),
    ("きぇ", "kye"),
    ("しゃ", "sha"),
    ("しゅ", "shu"),
    ("しょ", "sho"),
    ("しぇ", "she"),
    ("しゃ", "sya"),
    ("しゅ", "syu"),
    ("しょ", "syo"),
    ("ちゃ", "cha"),
    ("ちゅ", "chu"),
    ("ちょ", "cho"),
    ("ちぇ", "che"),
    ("ちゃ", "tya"),
    ("ちゅ", "tyu"),
    ("ちょ", "tyo"),
    ("にゃ", "nya"),
    ("にゅ", "nyu"),
    ("にょ", "nyo"),
    ("にぇ", "nye"),
    ("ひゃ", "hya"),
    ("ひゅ", "hyu"),
    ("ひょ", "hyo"),
    ("ひぇ", "hye"),
    ("みゃ", "mya"),
    ("みゅ", "myu"),
    ("みょ", "myo"),
    ("みぇ", "mye"),
    ("りゃ", "rya"),
    ("りゅ", "ryu"),
    ("りょ", "ryo"),
    ("りぇ", "rye"),
    ("ぎゃ", "gya"),
    ("ぎゅ", "gyu"),
    ("ぎょ", "gyo"),
    ("ぎぇ", "gye"),
    ("じゃ", "ja"),
    ("じゅ", "ju"),
    ("じょ", "jo"),
    ("じぇ", "je"),
    ("じゃ", "jya"),
    ("じゅ", "jyu"),
    ("じょ", "jyo"),
    ("じゃ", "zya"),
    ("じゅ", "zyu"),
    ("じょ", "zyo"),
    ("びゃ", "bya"),
    ("びゅ", "byu"),
    ("びょ", "byo"),
    ("びぇ", "bye"),
    ("ぴゃ", "pya"),
    ("ぴゅ", "pyu"),
    ("ぴょ", "pyo"),
    ("ぴぇ", "pye"),
    ("てぃ", "ti"),
    ("でぃ", "di"),
    ("とぅ", "tu"),
    ("どぅ", "du"),
    ("てゅ", "tyu"),
    ("でゅ", "dyu"),
    ("ふぁ", "fa"),
    ("ふぃ", "fi"),
    ("ふぇ", "fe"),
    ("ふぉ", "fo"),
    ("ふゅ", "fyu"),
    ("うぃ", "wi"),
    ("うぇ", "we"),
    ("うぉ", "who"),
    ("ゔぁ", "va"),
    ("ゔぃ", "vi"),
    ("ゔぇ", "ve"),
    ("ゔぉ", "vo"),
    ("ゔ", "vu"),
    ("つぁ", "tsa"),
    ("つぃ", "tsi"),
    ("つぇ", "tse"),
    ("つぉ", "tso"),
    ("いぇ", "ye"),
    ("すぃ", "si"),
    ("ずぃ", "zi"),
    ("あ", "a"),
    ("い", "i"),
    ("う", "u"),
    ("え", "e"),
    ("お", "o"),
    ("か", "ka"),
    ("き", "ki"),
    ("く", "ku"),
    ("け", "ke"),
    ("こ", "ko"),
    ("さ", "sa"),
    ("し", "shi"),
    ("す", "su"),
    ("せ", "se"),
    ("そ", "so"),
    ("た", "ta"),
    ("ち", "chi"),
    ("つ", "tsu"),
    ("て", "te"),
    ("と", "to"),
    ("な", "na"),
    ("に", "ni"),
    ("ぬ", "nu"),
    ("ね", "ne"),
    ("の", "no"),
    ("は", "ha"),
    ("ひ", "hi"),
    ("ふ", "fu"),
    ("へ", "he"),
    ("ほ", "ho"),
    ("ふ", "hu"),
    ("ま", "ma"),
    ("み", "mi"),
    ("む", "mu"),
    ("め", "me"),
    ("も", "mo"),
    ("や", "ya"),
    ("ゆ", "yu"),
    ("よ", "yo"),
    ("ら", "ra"),
    ("り", "ri"),
    ("る", "ru"),
    ("れ", "re"),
    ("ろ", "ro"),
    ("わ", "wa"),
    ("を", "wo"),
    ("ん", "n"),
    ("ん", "nn"),
    ("が", "ga"),
    ("ぎ", "gi"),
    ("ぐ", "gu"),
    ("げ", "ge"),
    ("ご", "go"),
    ("ざ", "za"),
    ("じ", "ji"),
    ("ず", "zu"),
    ("ぜ", "ze"),
    ("ぞ", "zo"),
    ("だ", "da"),
    ("ぢ", "dji"),
    ("づ", "dzu"),
    ("で", "de"),
    ("ど", "do"),
    ("ば", "ba"),
    ("び", "bi"),
    ("ぶ", "bu"),
    ("べ", "be"),
    ("ぼ", "bo"),
    ("ぱ", "pa"),
    ("ぴ", "pi"),
    ("ぷ", "pu"),
    ("ぺ", "pe"),
    ("ぽ", "po"),
    ("ぁ", "xa"),
    ("ぃ", "xi"),
    ("ぅ", "xu"),
    ("ぇ", "xe"),
    ("ぉ", "xo"),
    ("っ", "xtsu"),
    ("ー", "-"),
];

struct Maps {
    to_kana: HashMap<&'static str, &'static str>,
    to_romaji: HashMap<&'static str, &'static str>,
    longest: usize,
}

fn maps() -> &'static Maps {
    static MAPS: OnceLock<Maps> = OnceLock::new();
    MAPS.get_or_init(|| {
        let mut to_kana = HashMap::new();
        let mut to_romaji = HashMap::new();
        for &(k, r) in TABLE {
            to_kana.entry(r).or_insert(k);
            to_romaji.entry(k).or_insert(r);
        }
        // Unmistakable spellings that collide with modern ones above.
        to_romaji.insert("し", "shi");
        to_romaji.insert("ふ", "fu");
        to_romaji.insert("ち", "chi");
        to_romaji.insert("つ", "tsu");
        to_romaji.insert("じ", "ji");
        let longest = TABLE.iter().map(|(_, r)| r.len()).max().unwrap_or(1);
        Maps {
            to_kana,
            to_romaji,
            longest,
        }
    })
}

fn katakana_to_hiragana(c: char) -> char {
    match c as u32 {
        0x30A1..=0x30F6 => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    }
}

fn is_kana(c: char) -> bool {
    matches!(c as u32, 0x3041..=0x30FF)
}

/// Katakana and romaji to hiragana; text that is neither is left as it is.
pub fn to_hiragana(s: &str) -> String {
    let s = s.trim();
    if s.chars().any(is_kana) {
        return s.chars().map(katakana_to_hiragana).collect();
    }
    romaji_to_hiragana(&s.to_ascii_lowercase()).unwrap_or_else(|| s.to_string())
}

/// Romaji read fully as kana, or `None` when some of it is not romaji.
fn romaji_to_hiragana(s: &str) -> Option<String> {
    if s.is_empty() || !s.is_ascii() {
        return None;
    }
    let m = maps();
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        // A doubled consonant is a small tsu: "kka" → っか.
        if i + 1 < b.len() && b[i] == b[i + 1] && b"kstpcgzdbhfmjrw".contains(&b[i]) {
            out.push('っ');
            i += 1;
            continue;
        }
        let mut matched = false;
        for len in (1..=m.longest.min(b.len() - i)).rev() {
            let piece = &s[i..i + len];
            if piece == "-" {
                continue;
            }
            if let Some(k) = m.to_kana.get(piece) {
                // "n" before a vowel or y belongs to the next mora.
                if piece == "n" && i + 1 < b.len() && b"aiueoy".contains(&b[i + 1]) {
                    continue;
                }
                out.push_str(k);
                i += len;
                matched = true;
                break;
            }
        }
        if !matched {
            return None;
        }
    }
    Some(out)
}

/// "か" → "ka", "きゃ" → "kya", "ん" → "n".
pub fn kana_to_romaji(kana: &str) -> Option<String> {
    let h: String = kana.chars().map(katakana_to_hiragana).collect();
    maps().to_romaji.get(h.as_str()).map(|r| r.to_string())
}

/// A romanised mora's consonant and vowel: "kya" → ("ky", "a"),
/// "a" → ("", "a"), "n" → ("", "n").
pub fn split_romaji(romaji: &str) -> (String, String) {
    if romaji == "n" || romaji == "nn" {
        return (String::new(), "n".into());
    }
    match romaji.char_indices().last() {
        Some((i, c)) if "aiueo".contains(c) => (romaji[..i].to_string(), c.to_string()),
        _ => (romaji.to_string(), String::new()),
    }
}

/// The vowel a mora ends on: a, i, u, e, o, or n.
pub fn vowel_of(mora: &str) -> Option<&'static str> {
    let h = to_hiragana(mora);
    let r = kana_to_romaji(&h).or_else(|| Some(mora.to_ascii_lowercase()))?;
    let (_, v) = split_romaji(&r);
    ["a", "i", "u", "e", "o", "n"].into_iter().find(|x| *x == v)
}

/// Hiragana split into morae, small kana kept with the one before.
pub fn morae(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in s.chars() {
        let small = "ぁぃぅぇぉゃゅょゎァィゥェォャュョヮ".contains(c);
        match out.last_mut() {
            Some(last) if small => last.push(c),
            _ => out.push(c.to_string()),
        }
    }
    out
}
