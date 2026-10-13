//! Typing lyrics: a line spread over notes, and the syllable a new note sings.
//!
//! The rules are the singer ABI's (`fontelle_singer_abi`'s crate doc), so
//! what the writer types is what the engine reads:
//!
//! - words are separated by spaces, one note each;
//! - `"-"` on its own is a slur, a note of its own that carries the last
//!   syllable on;
//! - a word with hyphens inside (`"a-ma-zing"`) is a syllable per note, each
//!   but the last keeping its hyphen (`"a-" "ma-" "zing"`) so the engine
//!   still knows they are one word;
//! - phonemes in brackets (`"[h ə l oʊ]"`) are one lyric, spaces and all;
//! - kana are a mora per note (`"さくら"` is `"さ" "く" "ら"`), small kana
//!   joining the one before (`"きょ"`), because that is how Japanese is sung
//!   and written under notes;
//! - Chinese characters and Korean hangul are a syllable per note
//!   (`"我爱你"` is `"我" "爱" "你"`, `"사랑해"` is `"사" "랑" "해"`), a
//!   reading in brackets staying with its character (`"银行[hang]"` is
//!   `"银" "行[hang]"`) and punctuation with the character before it.

/// What [`spread`] made of a line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Spread {
    /// One lyric per note, for the first `lyrics.len()` notes in time order;
    /// never more than the notes there were.
    pub lyrics: Vec<String>,
    /// How many lyrics the line had past the last note — for the status bar
    /// to say so rather than drop words quietly.
    pub left_over: usize,
}

/// Spreads `line` over `notes` notes. See the module doc for the rules.
pub fn spread(line: &str, notes: usize) -> Spread {
    let tokens = tokens(line);
    let left_over = tokens.len().saturating_sub(notes);
    let mut lyrics = tokens;
    lyrics.truncate(notes);
    Spread { lyrics, left_over }
}

/// Every lyric in `line`, in order.
pub fn tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line.trim();
    while !rest.is_empty() {
        if rest.starts_with('[') {
            // Phonemes: to the closing bracket, spaces and all.
            let end = rest.find(']').map_or(rest.len(), |i| i + 1);
            out.push(rest[..end].to_string());
            rest = rest[end..].trim_start();
            continue;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        word(&rest[..end], &mut out);
        rest = rest[end..].trim_start();
    }
    out
}

fn word(word: &str, out: &mut Vec<String>) {
    if word == fontelle_lyric_slur() {
        out.push(word.to_string());
        return;
    }
    if word.chars().all(is_kana) {
        morae(word, out);
        return;
    }
    if let Some(syllables) = cjk_syllables(word) {
        out.extend(syllables);
        return;
    }
    // "a-ma-zing" → "a-" "ma-" "zing"; "hel-" stays "hel-". Hyphens at the
    // front or doubled are kept as written rather than guessed at.
    let parts: Vec<&str> = word.split('-').collect();
    if parts.len() < 2 || parts.iter().any(|p| p.is_empty()) {
        out.push(word.to_string());
        return;
    }
    for (i, part) in parts.iter().enumerate() {
        if i + 1 < parts.len() {
            out.push(format!("{part}-"));
        } else {
            out.push((*part).to_string());
        }
    }
}

/// The ABI's slur, spelled here so the model need not depend on the ABI
/// crate for one character.
fn fontelle_lyric_slur() -> &'static str {
    "-"
}

fn is_kana(c: char) -> bool {
    ('\u{3041}'..='\u{309F}').contains(&c) || ('\u{30A0}'..='\u{30FF}').contains(&c)
}

/// A Chinese character or a hangul syllable block: a syllable each.
fn is_cjk_syllable(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x323AF
            | 0xAC00..=0xD7A3)
}

/// A word of Chinese characters or hangul, a syllable each, with any
/// bracketed reading or punctuation after a syllable kept on it; `None`
/// when the word has anything else in it.
fn cjk_syllables(word: &str) -> Option<Vec<String>> {
    if !word.chars().any(is_cjk_syllable) {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    let mut lead = String::new();
    let mut in_bracket = false;
    for c in word.chars() {
        if in_bracket {
            out.last_mut()?.push(c);
            in_bracket = c != ']';
        } else if is_cjk_syllable(c) {
            out.push(std::mem::take(&mut lead));
            out.last_mut()?.push(c);
        } else if c == '[' {
            out.last_mut()?.push(c);
            in_bracket = true;
        } else if !c.is_alphanumeric() {
            // Punctuation: on the syllable before, or the first one.
            match out.last_mut() {
                Some(last) => last.push(c),
                None => lead.push(c),
            }
        } else {
            return None;
        }
    }
    Some(out)
}

/// Small kana that belong to the mora before them.
fn joins_previous(c: char) -> bool {
    matches!(
        c,
        'ゃ' | 'ゅ'
            | 'ょ'
            | 'ぁ'
            | 'ぃ'
            | 'ぅ'
            | 'ぇ'
            | 'ぉ'
            | 'ゎ'
            | 'ャ'
            | 'ュ'
            | 'ョ'
            | 'ァ'
            | 'ィ'
            | 'ゥ'
            | 'ェ'
            | 'ォ'
            | 'ヮ'
    )
}

fn morae(word: &str, out: &mut Vec<String>) {
    let mut current = String::new();
    for c in word.chars() {
        if joins_previous(c) && !current.is_empty() {
            current.push(c);
            continue;
        }
        if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        current.push(c);
    }
    if !current.is_empty() {
        out.push(current);
    }
}

/// The syllable a note in `language` sings when nobody has written one —
/// what a new note on a vocal channel is given, so it shows and sings
/// something rather than nothing.
pub fn default_syllable(language: &str) -> &'static str {
    let primary = language.split(['-', '_']).next().unwrap_or("");
    match primary {
        "ja" => "ら",
        "ko" => "라",
        _ => "la",
    }
}
