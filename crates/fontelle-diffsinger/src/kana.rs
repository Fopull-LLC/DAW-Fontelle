//! Kana lyrics as the romaji Japanese DiffSinger dictionaries are keyed by
//! (Hepburn: `shi`, `tsu`, `fu`, `ji`; `cl` for a geminate, `N` for ん).

/// A lyric's morae in romaji, or `None` when it is not all kana.
pub fn morae(text: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = text.trim().chars().map(to_hiragana).collect();
    if chars.is_empty() {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if let Some(&next) = chars.get(i + 1)
            && let Some(r) = digraph(chars[i], next)
        {
            out.push(r.to_string());
            i += 2;
            continue;
        }
        let r = match chars[i] {
            'ー' => {
                let vowel = out
                    .last()?
                    .chars()
                    .last()
                    .filter(|c| "aiueo".contains(*c))?;
                vowel.to_string()
            }
            c => single(c)?.to_string(),
        };
        out.push(r);
        i += 1;
    }
    Some(out)
}

fn to_hiragana(c: char) -> char {
    match c {
        'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    }
}

fn single(c: char) -> Option<&'static str> {
    Some(match c {
        'あ' | 'ぁ' => "a",
        'い' | 'ぃ' | 'ゐ' => "i",
        'う' | 'ぅ' => "u",
        'え' | 'ぇ' | 'ゑ' => "e",
        'お' | 'ぉ' | 'を' => "o",
        'か' | 'ゕ' => "ka",
        'き' => "ki",
        'く' => "ku",
        'け' | 'ゖ' => "ke",
        'こ' => "ko",
        'が' => "ga",
        'ぎ' => "gi",
        'ぐ' => "gu",
        'げ' => "ge",
        'ご' => "go",
        'さ' => "sa",
        'し' => "shi",
        'す' => "su",
        'せ' => "se",
        'そ' => "so",
        'ざ' => "za",
        'じ' | 'ぢ' => "ji",
        'ず' | 'づ' => "zu",
        'ぜ' => "ze",
        'ぞ' => "zo",
        'た' => "ta",
        'ち' => "chi",
        'つ' => "tsu",
        'て' => "te",
        'と' => "to",
        'だ' => "da",
        'で' => "de",
        'ど' => "do",
        'な' => "na",
        'に' => "ni",
        'ぬ' => "nu",
        'ね' => "ne",
        'の' => "no",
        'は' => "ha",
        'ひ' => "hi",
        'ふ' => "fu",
        'へ' => "he",
        'ほ' => "ho",
        'ば' => "ba",
        'び' => "bi",
        'ぶ' => "bu",
        'べ' => "be",
        'ぼ' => "bo",
        'ぱ' => "pa",
        'ぴ' => "pi",
        'ぷ' => "pu",
        'ぺ' => "pe",
        'ぽ' => "po",
        'ま' => "ma",
        'み' => "mi",
        'む' => "mu",
        'め' => "me",
        'も' => "mo",
        'や' | 'ゃ' => "ya",
        'ゆ' | 'ゅ' => "yu",
        'よ' | 'ょ' => "yo",
        'ら' => "ra",
        'り' => "ri",
        'る' => "ru",
        'れ' => "re",
        'ろ' => "ro",
        'わ' | 'ゎ' => "wa",
        'ん' => "N",
        'っ' => "cl",
        'ゔ' => "vu",
        _ => return None,
    })
}

fn digraph(a: char, b: char) -> Option<&'static str> {
    Some(match (a, b) {
        ('き', 'ゃ') => "kya",
        ('き', 'ゅ') => "kyu",
        ('き', 'ぇ') => "kye",
        ('き', 'ょ') => "kyo",
        ('ぎ', 'ゃ') => "gya",
        ('ぎ', 'ゅ') => "gyu",
        ('ぎ', 'ぇ') => "gye",
        ('ぎ', 'ょ') => "gyo",
        ('し', 'ゃ') => "sha",
        ('し', 'ゅ') => "shu",
        ('し', 'ぇ') => "she",
        ('し', 'ょ') => "sho",
        ('じ', 'ゃ') | ('ぢ', 'ゃ') => "ja",
        ('じ', 'ゅ') | ('ぢ', 'ゅ') => "ju",
        ('じ', 'ぇ') | ('ぢ', 'ぇ') => "je",
        ('じ', 'ょ') | ('ぢ', 'ょ') => "jo",
        ('ち', 'ゃ') => "cha",
        ('ち', 'ゅ') => "chu",
        ('ち', 'ぇ') => "che",
        ('ち', 'ょ') => "cho",
        ('に', 'ゃ') => "nya",
        ('に', 'ゅ') => "nyu",
        ('に', 'ぇ') => "nye",
        ('に', 'ょ') => "nyo",
        ('ひ', 'ゃ') => "hya",
        ('ひ', 'ゅ') => "hyu",
        ('ひ', 'ぇ') => "hye",
        ('ひ', 'ょ') => "hyo",
        ('び', 'ゃ') => "bya",
        ('び', 'ゅ') => "byu",
        ('び', 'ぇ') => "bye",
        ('び', 'ょ') => "byo",
        ('ぴ', 'ゃ') => "pya",
        ('ぴ', 'ゅ') => "pyu",
        ('ぴ', 'ぇ') => "pye",
        ('ぴ', 'ょ') => "pyo",
        ('み', 'ゃ') => "mya",
        ('み', 'ゅ') => "myu",
        ('み', 'ぇ') => "mye",
        ('み', 'ょ') => "myo",
        ('り', 'ゃ') => "rya",
        ('り', 'ゅ') => "ryu",
        ('り', 'ぇ') => "rye",
        ('り', 'ょ') => "ryo",
        ('ふ', 'ぁ') => "fa",
        ('ふ', 'ぃ') => "fi",
        ('ふ', 'ぇ') => "fe",
        ('ふ', 'ぉ') => "fo",
        ('ふ', 'ゅ') => "fyu",
        ('て', 'ぃ') => "ti",
        ('て', 'ゅ') => "tyu",
        ('で', 'ぃ') => "di",
        ('で', 'ゅ') => "dyu",
        ('と', 'ぅ') => "tu",
        ('ど', 'ぅ') => "du",
        ('う', 'ぃ') => "wi",
        ('う', 'ぇ') => "we",
        ('う', 'ぉ') => "wo",
        ('い', 'ぇ') => "ye",
        ('つ', 'ぁ') => "tsa",
        ('つ', 'ぃ') => "tsi",
        ('つ', 'ぇ') => "tse",
        ('つ', 'ぉ') => "tso",
        ('く', 'ぁ') | ('く', 'ゎ') => "kwa",
        ('ぐ', 'ぁ') | ('ぐ', 'ゎ') => "gwa",
        ('ゔ', 'ぁ') => "va",
        ('ゔ', 'ぃ') => "vi",
        ('ゔ', 'ぇ') => "ve",
        ('ゔ', 'ぉ') => "vo",
        _ => return None,
    })
}
