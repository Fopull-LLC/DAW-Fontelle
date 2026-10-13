//! Chinese characters to toneless pinyin.
//!
//! **The readings** (`data/pinyin.txt`) are each character's most common
//! reading: Unicode's Unihan `kMandarin` by way of mozillazg's
//! `pinyin-data` (MIT; Unihan under Unicode-3.0), corrected by its
//! `kMandarin_overwrite` and, for the 8105 everyday characters, by the
//! 2013 Table of General Standard Chinese Characters' most common reading
//! (`kMandarin_8105`). `data/make_pinyin.py` rebuilds it; the notices are in
//! `licenses/pinyin-data/`. Tones are dropped — singing voicebanks are keyed
//! by toneless pinyin — and ü is `v` after `l`/`n` (`lv`), `u` elsewhere
//! (`qu`), as the Opencpop-style dictionaries DiffSinger banks ship do.
//!
//! **Polyphonic characters** read their commonest reading, except
//! - in a common word that reads them otherwise (`data/pinyin-words.txt`,
//!   written here: `银行` is `yin hang`), matched across a run of notes, so
//!   a word sung a character a note is still the word;
//! - where songs use another reading far more than prose does
//!   ([`SUNG`]: `长` is `chang`);
//! - and always where the writer put a reading in brackets after the
//!   character: `行[hang]` (tones and `ü` accepted, `行[háng]`,
//!   `绿[lü4]`).

use std::collections::HashMap;
use std::sync::OnceLock;

const READINGS: &str = include_str!("../data/pinyin.txt");
const WORDS: &str = include_str!("../data/pinyin-words.txt");

/// Characters whose commonest sung reading is not their commonest one.
const SUNG: &[(char, &str)] = &[('长', "chang"), ('似', "si")];

/// The longest word in the word list, in characters.
const LONGEST_WORD: usize = 4;

struct Tables {
    chars: HashMap<char, &'static str>,
    words: HashMap<Vec<char>, Vec<&'static str>>,
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut chars = HashMap::with_capacity(45_000);
        for line in READINGS.lines() {
            if let Some((reading, cs)) = line.split_once('\t') {
                for c in cs.chars() {
                    chars.insert(c, reading);
                }
            }
        }
        for (c, r) in SUNG {
            chars.insert(*c, *r);
        }
        let mut words = HashMap::new();
        for line in WORDS.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let Some(word) = parts.next() else { continue };
            let readings: Vec<&'static str> = parts.collect();
            let word: Vec<char> = word.chars().collect();
            if word.len() == readings.len() && word.len() <= LONGEST_WORD {
                words.insert(word, readings);
            }
        }
        Tables { chars, words }
    })
}

/// Whether `c` is a Chinese character (CJK Unified Ideographs, extensions,
/// compatibility ideographs).
pub fn is_hanzi(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2EE5F
            | 0x2F800..=0x2FA1F
            | 0x30000..=0x323AF
            | 0x3007)
}

/// Whether `text` has a Chinese character in it.
pub fn has_hanzi(text: &str) -> bool {
    text.chars().any(is_hanzi)
}

/// The character's most common reading, toneless.
pub fn reading(c: char) -> Option<&'static str> {
    tables().chars.get(&c).copied()
}

/// What one lyric says, a syllable per character; `None` when it is not
/// Chinese characters (with optional bracketed readings) throughout.
pub fn read(text: &str) -> Option<Vec<String>> {
    read_run(&[text]).pop().flatten()
}

fn is_punctuation(c: char) -> bool {
    c.is_ascii_punctuation() && c != '[' && c != ']'
        || c.is_whitespace()
        || "…“”‘’「」『』、。！？，；：（）《》".contains(c)
}

/// A typed reading, normalised: tones (marks or digits) dropped, `ü`/`u:`
/// as `v` after `l`/`n` and `u` elsewhere.
fn normalise(typed: &str) -> Option<String> {
    let mut out = String::new();
    for c in typed.trim().chars() {
        let base = match c {
            'ā' | 'á' | 'ǎ' | 'à' => 'a',
            'ē' | 'é' | 'ě' | 'è' | 'ê' => 'e',
            'ī' | 'í' | 'ǐ' | 'ì' => 'i',
            'ō' | 'ó' | 'ǒ' | 'ò' => 'o',
            'ū' | 'ú' | 'ǔ' | 'ù' => 'u',
            'ü' | 'ǖ' | 'ǘ' | 'ǚ' | 'ǜ' => 'v',
            'ń' | 'ň' | 'ǹ' => 'n',
            'ḿ' => 'm',
            '0'..='9' | ':' => continue,
            c if c.is_ascii_alphabetic() => c.to_ascii_lowercase(),
            _ => return None,
        };
        out.push(base);
    }
    let out = out.replace("u:", "v");
    let out = match out.get(..2) {
        Some("jv" | "qv" | "xv" | "yv") => format!("{}u{}", &out[..1], &out[2..]),
        _ => out,
    };
    (!out.is_empty()).then_some(out)
}

/// A character and the reading written after it, if one was.
type Written = (char, Option<String>);

/// A lyric's characters and the reading written after any of them, or
/// `None` when it is not Chinese throughout.
fn characters(lyric: &str) -> Option<Vec<Written>> {
    let mut out: Vec<Written> = Vec::new();
    let mut rest = lyric.trim_matches(|c: char| is_punctuation(c));
    while let Some(c) = rest.chars().next() {
        if c == '[' {
            let end = rest.find(']')?;
            let reading = normalise(&rest[1..end])?;
            out.last_mut()?.1 = Some(reading);
            rest = &rest[end + 1..];
        } else if is_hanzi(c) {
            out.push((c, None));
            rest = &rest[c.len_utf8()..];
        } else if is_punctuation(c) {
            rest = &rest[c.len_utf8()..];
        } else {
            return None;
        }
    }
    (!out.is_empty()).then_some(out)
}

/// What each lyric of a run of notes says, read together so a word sung
/// a character a note (`银` `行`) is still found as the word.
pub fn read_run(lyrics: &[&str]) -> Vec<Option<Vec<String>>> {
    let parsed: Vec<Option<Vec<Written>>> = lyrics.iter().map(|l| characters(l)).collect();
    let t = tables();
    let mut out: Vec<Option<Vec<String>>> = Vec::with_capacity(lyrics.len());
    let mut i = 0;
    while i < parsed.len() {
        if parsed[i].is_none() {
            out.push(None);
            i += 1;
            continue;
        }
        let mut j = i;
        while j < parsed.len() && parsed[j].is_some() {
            j += 1;
        }
        let flat: Vec<&Written> = parsed[i..j].iter().flatten().flatten().collect();
        let chars: Vec<char> = flat.iter().map(|(c, _)| *c).collect();
        // Each character's reading: the word's, the longest first.
        let mut read: Vec<Option<&str>> = vec![None; chars.len()];
        let mut k = 0;
        while k < chars.len() {
            let found = (2..=LONGEST_WORD.min(chars.len() - k))
                .rev()
                .find_map(|n| t.words.get(&chars[k..k + n]).map(|r| (n, r)));
            match found {
                Some((n, readings)) => {
                    for (m, r) in readings.iter().enumerate() {
                        read[k + m] = Some(r);
                    }
                    k += n;
                }
                None => {
                    read[k] = reading(chars[k]);
                    k += 1;
                }
            }
        }
        let syllables: Vec<Option<String>> = flat
            .iter()
            .zip(&read)
            .map(|((_, typed), r)| typed.clone().or_else(|| r.map(str::to_string)))
            .collect();
        let mut at = 0;
        for p in &parsed[i..j] {
            let n = p.as_ref().map_or(0, Vec::len);
            let mine: Option<Vec<String>> = syllables[at..at + n].iter().cloned().collect();
            out.push(mine);
            at += n;
        }
        i = j;
    }
    out
}

/// Pinyin's initials, two-letter ones first.
const INITIALS: [&str; 23] = [
    "zh", "ch", "sh", "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x", "r",
    "z", "c", "s", "y", "w",
];

/// Whether `s` is a pinyin syllable (toneless, `v` for ü).
pub fn is_syllable(s: &str) -> bool {
    READINGS
        .lines()
        .any(|l| l.split_once('\t').is_some_and(|(r, _)| r == s))
}

/// A syllable's initial and final: `zhuang` → (`zh`, `uang`), `ai` →
/// (``, `ai`), `yu` → (`y`, `u`).
pub fn split(syllable: &str) -> (&str, &str) {
    for i in INITIALS {
        if let Some(rest) = syllable.strip_prefix(i)
            && !rest.is_empty()
        {
            return (&syllable[..i.len()], rest);
        }
    }
    ("", syllable)
}
