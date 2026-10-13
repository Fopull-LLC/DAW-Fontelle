//! A phrase's lyrics as words: which notes each word is sung over and what
//! it says. The rules are `fontelle-singer-abi`'s (see its note on lyrics):
//!
//! - a word or syllable is looked up in the bank's dictionary;
//! - `-` carries the previous note's word on to this pitch; a word of
//!   several syllables followed by slurs is spread over them, a syllable a
//!   note, and slurs past its last syllable hold it;
//! - a slur after a rest (the first note, or one not touching the note
//!   before) sings the previous syllable's vowel again;
//! - a syllable ending in a hyphen joins the next note's into one word
//!   (`a-` `ma-` `zing` is "amazing" over three notes);
//! - `[h ə l oʊ]` is phonemes, used as given;
//! - empty is the voice's default syllable.

use fontelle_sing::{LYRIC_SLUR, PhraseNote};

/// How close two notes must be, in seconds, to count as touching.
pub const TOUCH: f64 = 1e-3;

/// What a word says.
#[derive(Debug, Clone, PartialEq)]
pub enum Reading {
    /// Text to look up.
    Text(String),
    /// Phonemes the writer gave in brackets.
    Phonemes(Vec<String>),
    /// The voice's default syllable.
    Default,
    /// The previous word's last vowel, sung again.
    Resing,
}

/// One word and the notes it is sung over, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub notes: Vec<usize>,
    pub reading: Reading,
}

pub fn words_of(notes: &[PhraseNote]) -> Vec<Word> {
    let mut words: Vec<Word> = Vec::new();
    // The last word is waiting for the next note's syllable.
    let mut joining = false;
    for (i, note) in notes.iter().enumerate() {
        let lyric = note.lyric.trim();
        let touches = i > 0 && note.start - notes[i - 1].end() <= TOUCH;
        if lyric == LYRIC_SLUR {
            match words.last_mut() {
                Some(word) if touches || joining => word.notes.push(i),
                _ => {
                    joining = false;
                    words.push(Word {
                        notes: vec![i],
                        reading: Reading::Resing,
                    });
                }
            }
            continue;
        }
        let (syllable, joins_next) = match lyric.strip_suffix('-') {
            Some(s) if !s.is_empty() && !lyric.starts_with('[') => (s, true),
            _ => (lyric, false),
        };
        if joining
            && let Some(word) = words.last_mut()
            && let Reading::Text(text) = &mut word.reading
        {
            text.push_str(syllable);
            word.notes.push(i);
            joining = joins_next;
            continue;
        }
        joining = joins_next;
        let reading = if syllable.is_empty() {
            Reading::Default
        } else if let Some(inner) = syllable.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            Reading::Phonemes(inner.split_whitespace().map(str::to_string).collect())
        } else {
            Reading::Text(syllable.to_string())
        };
        words.push(Word {
            notes: vec![i],
            reading,
        });
    }
    words
}
