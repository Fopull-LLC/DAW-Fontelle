//! Words to phonemes, and phonemes to notes.
//!
//! A word's phonemes are spread over its notes the way DiffSinger banks are
//! trained: each vowel (or, in consonant–glide–vowel, the glide) is where a
//! note begins; the consonants before a word's first vowel are sung before
//! its first note starts; when the notes run out the rest stay on the last
//! one, and notes beyond the vowels hold the last vowel.

use fontelle_sing::{PhonemeEdit, PhraseNote};

use crate::dict::Dictionary;
use crate::inventory::Inventory;
use crate::lyrics::{Reading, Word};

/// A phoneme, and where the writer put it if they did.
#[derive(Debug, Clone, PartialEq)]
pub struct Ph {
    /// The bank's own symbol (`en/hh` in a multi-language bank).
    pub symbol: String,
    /// Seconds from its note's start, when placed by hand.
    pub offset: Option<f64>,
}

impl Ph {
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            offset: None,
        }
    }
}

/// What one note sings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NotePhonemes {
    /// Sung before the note starts (a syllable's onset).
    pub lead: Vec<Ph>,
    /// Sung from the note's start.
    pub body: Vec<Ph>,
    /// The word was not in the dictionary and its reading is a guess.
    pub guessed: bool,
}

impl NotePhonemes {
    pub fn is_empty(&self) -> bool {
        self.lead.is_empty() && self.body.is_empty()
    }
}

/// Reads words in one language with one bank's dictionary and inventory.
pub struct Reader<'a> {
    pub dict: &'a Dictionary,
    pub inventory: &'a Inventory,
    /// The language code symbols are prefixed with in a multi-language bank.
    pub language: &'a str,
}

impl Reader<'_> {
    /// The bank's symbol for what the writer typed: as given, or with the
    /// language's prefix (`hh` is `en/hh` in English).
    pub fn resolve(&self, symbol: &str) -> Option<String> {
        let known = |s: &str| self.inventory.contains(s) && self.dict.has_symbol(s);
        if known(symbol) {
            return Some(symbol.to_string());
        }
        if !symbol.contains('/') && !self.language.is_empty() {
            let prefixed = format!("{}/{symbol}", self.language);
            if known(&prefixed) {
                return Some(prefixed);
            }
        }
        None
    }

    /// A word's phonemes, and whether they are a guess.
    pub fn read(&self, text: &str) -> (Vec<String>, bool) {
        self.read_with(text, None)
    }

    /// A word's phonemes, given how its run of notes reads it when it is
    /// Chinese or Korean (words across notes, a coda carried over).
    pub fn read_with(&self, text: &str, context: Option<&Script>) -> (Vec<String>, bool) {
        let text = text.trim();
        if let Some(p) = self.lookup(text) {
            return (p, false);
        }
        let bare = text.trim_matches(|c: char| PUNCTUATION.contains(c));
        if let Some(p) = self.lookup(bare) {
            return (p, false);
        }
        match context
            .cloned()
            .or_else(|| Script::of(text))
            .or_else(|| Script::of(bare))
        {
            Some(Script::Pinyin(s)) => return self.read_pinyin(&s),
            Some(Script::Hangul(s)) => return self.read_hangul(&s),
            None => {}
        }
        // Kana, a mora at a time, in the romaji the dictionary is keyed by.
        if let Some(morae) = crate::kana::morae(bare) {
            let read: Option<Vec<Vec<String>>> = morae
                .iter()
                .map(|m| self.lookup(m).or_else(|| self.lookup(&m.to_lowercase())))
                .collect();
            if let Some(p) = read {
                return (p.concat(), false);
            }
        }
        // Phonemes typed without brackets.
        let typed: Option<Vec<String>> = bare.split_whitespace().map(|s| self.resolve(s)).collect();
        if let Some(p) = typed.filter(|p| !p.is_empty()) {
            return (p, false);
        }
        if let Some(p) = self.english(bare) {
            return (p, false);
        }
        if let Some(p) = self.compound(&bare.to_lowercase()) {
            return (p, true);
        }
        (self.default_syllable(), true)
    }

    /// Pinyin syllables: each as the dictionary has it, else as its
    /// initial and final when the bank has those symbols.
    fn read_pinyin(&self, syllables: &[String]) -> (Vec<String>, bool) {
        let mut out = Vec::new();
        let mut guessed = false;
        for s in syllables {
            if let Some(p) = self.lookup(s) {
                out.extend(p);
                continue;
            }
            let (i, f) = fontelle_lang::pinyin::split(s);
            let parts: Option<Vec<String>> = [i, f]
                .iter()
                .filter(|x| !x.is_empty())
                .map(|x| self.resolve(x))
                .collect();
            match parts {
                Some(p) if !p.is_empty() => out.extend(p),
                _ => {
                    guessed = true;
                    out.extend(self.default_syllable());
                }
            }
        }
        (out, guessed)
    }

    /// Hangul syllables: each as the dictionary has the block or its
    /// romanisation, else as onset, vowel and coda symbols.
    fn read_hangul(&self, syllables: &[fontelle_lang::hangul::Syllable]) -> (Vec<String>, bool) {
        let mut out = Vec::new();
        let mut guessed = false;
        for s in syllables {
            if let Some(p) = self
                .lookup(&s.roman())
                .or_else(|| self.lookup(&s.text.to_string()))
            {
                out.extend(p);
                continue;
            }
            let sound = |x: &str| self.resolve(x).or_else(|| self.resolve(&x.to_uppercase()));
            let vowel = sound(s.vowel);
            let Some(vowel) = vowel else {
                guessed = true;
                out.extend(self.default_syllable());
                continue;
            };
            if !s.onset.is_empty() {
                match sound(s.onset) {
                    Some(o) => out.push(o),
                    None => guessed = true,
                }
            }
            out.push(vowel);
            if !s.coda.is_empty() {
                match sound(s.coda) {
                    Some(c) => out.push(c),
                    None => guessed = true,
                }
            }
        }
        (out, guessed)
    }

    /// An English word the bank's dictionary lacks, from CMUdict, when the
    /// bank has every sound in it: SPORE's dictionary names its symbols and
    /// no words, Nishiren's has no "world".
    fn english(&self, word: &str) -> Option<Vec<String>> {
        if self.language != "en" {
            return None;
        }
        let phones = fontelle_utau::dict::cmudict().lookup(word)?;
        phones
            .iter()
            .map(|p| {
                // The schwa is "ah" in banks without "ax", and back.
                let other = match p.as_str() {
                    "ax" => Some("ah"),
                    "ah" => Some("ax"),
                    _ => None,
                };
                self.resolve(p)
                    .or_else(|| other.and_then(|o| self.resolve(o)))
            })
            .collect()
    }

    /// What an empty lyric sings.
    pub fn default_syllable(&self) -> Vec<String> {
        for word in ["la", "ら", "ra", "a", "あ"] {
            if let Some(p) = self.lookup(word) {
                return p;
            }
        }
        // Any vowel of the language, else any vowel at all.
        let vowels: Vec<String> = self
            .dict
            .symbols()
            .into_iter()
            .filter(|s| self.dict.is_vowel(s) && s != "SP" && s != "AP")
            .filter(|s| self.inventory.contains(s))
            .collect();
        let prefix = format!("{}/", self.language);
        vowels
            .iter()
            .find(|s| s.starts_with(&prefix))
            .or(vowels.first())
            .map(|v| vec![v.clone()])
            .unwrap_or_else(|| vec!["SP".to_string()])
    }

    /// A dictionary word, as the bank's symbols; `None` when the word is
    /// not there or says nothing this voice can sing.
    fn lookup(&self, word: &str) -> Option<Vec<String>> {
        if word.is_empty() {
            return None;
        }
        let entry = self.dict.query(word).or_else(|| {
            (!self.language.is_empty())
                .then(|| self.dict.query(&format!("{}/{word}", self.language)))
                .flatten()
        })?;
        let p: Vec<String> = entry.iter().filter_map(|s| self.resolve(s)).collect();
        (!p.is_empty()).then_some(p)
    }

    /// A word made of dictionary words ("starlight"), in as few as possible.
    fn compound(&self, word: &str) -> Option<Vec<String>> {
        const MAX_PARTS: usize = 4;
        let cuts: Vec<usize> = word
            .char_indices()
            .map(|(i, _)| i)
            .chain([word.len()])
            .collect();
        let n = cuts.len() - 1;
        if n < 4 {
            return None;
        }
        // best[i]: the fewest parts covering word[..cuts[i]], and the cut
        // before the last of them.
        let mut best: Vec<Option<(usize, usize)>> = vec![None; n + 1];
        best[0] = Some((0, 0));
        for end in 2..=n {
            for start in 0..=end - 2 {
                let Some((parts, _)) = best[start] else {
                    continue;
                };
                if parts >= MAX_PARTS || self.lookup(&word[cuts[start]..cuts[end]]).is_none() {
                    continue;
                }
                if best[end].is_none_or(|(p, _)| parts + 1 < p) {
                    best[end] = Some((parts + 1, start));
                }
            }
        }
        let (parts, _) = best[n]?;
        if parts < 2 {
            return None;
        }
        let mut pieces = Vec::new();
        let mut end = n;
        while end > 0 {
            let (_, start) = best[end]?;
            pieces.push(self.lookup(&word[cuts[start]..cuts[end]])?);
            end = start;
        }
        Some(pieces.into_iter().rev().flatten().collect())
    }

    pub fn is_vowel(&self, symbol: &str) -> bool {
        self.dict.is_vowel(symbol)
    }
}

/// What each note of a phrase sings, from its words and the writer's own
/// phonemes (which replace the reading of their note).
pub fn phonemize(notes: &[PhraseNote], words: &[Word], reader: &Reader) -> Vec<NotePhonemes> {
    let mut out = vec![NotePhonemes::default(); notes.len()];
    let mut last_vowel: Option<String> = None;
    let scripts = Script::of_run(notes, words);
    for (word, script) in words.iter().zip(&scripts) {
        let (symbols, guessed) = match &word.reading {
            Reading::Text(text) => reader.read_with(text, script.as_ref()),
            Reading::Phonemes(typed) => {
                let p: Vec<String> = typed.iter().filter_map(|s| reader.resolve(s)).collect();
                if p.is_empty() {
                    (reader.default_syllable(), true)
                } else {
                    let dropped = p.len() < typed.len();
                    (p, dropped)
                }
            }
            Reading::Default => (reader.default_syllable(), false),
            Reading::Resing => match &last_vowel {
                Some(v) => (vec![v.clone()], false),
                None => (reader.default_syllable(), false),
            },
        };
        if let Some(v) = symbols.iter().rev().find(|s| reader.is_vowel(s)) {
            last_vowel = Some(v.clone());
        }
        for (note, mut ph) in distribute(&symbols, &word.notes, reader) {
            if let Some(slot) = out.get_mut(note) {
                ph.guessed = guessed;
                *slot = ph;
            }
        }
    }
    for (i, note) in notes.iter().enumerate() {
        if let Some(edits) = &note.phonemes {
            out[i] = edited(edits, reader);
        }
    }
    out
}

/// The writer's phonemes for one note, split at the first vowel.
pub fn edited(edits: &[PhonemeEdit], reader: &Reader) -> NotePhonemes {
    let mut out = NotePhonemes::default();
    let mut in_body = false;
    for edit in edits {
        let Some(symbol) = reader.resolve(edit.symbol.trim()) else {
            out.guessed = true;
            continue;
        };
        in_body |= reader.is_vowel(&symbol);
        let ph = Ph {
            symbol,
            offset: edit.offset,
        };
        if in_body {
            out.body.push(ph);
        } else {
            out.lead.push(ph);
        }
    }
    // Consonants alone: they are the note, not a lead-in to it.
    if out.body.is_empty() {
        out.body = std::mem::take(&mut out.lead);
    }
    out
}

/// A word's phonemes over its notes: each syllable start opens the next
/// note.
fn distribute(symbols: &[String], notes: &[usize], reader: &Reader) -> Vec<(usize, NotePhonemes)> {
    let n = symbols.len();
    let vowel: Vec<bool> = symbols.iter().map(|s| reader.is_vowel(s)).collect();
    let mut starts = vec![false; n];
    if !vowel.iter().any(|v| *v) {
        if n > 0 {
            starts[0] = true;
        }
    } else {
        for i in 0..n {
            if !vowel[i] {
                continue;
            }
            // Consonant–glide–vowel: the glide begins the note.
            if i >= 2 && reader.dict.is_glide(&symbols[i - 1]) && !vowel[i - 2] {
                starts[i - 1] = true;
            } else {
                starts[i] = true;
            }
        }
    }
    let mut out: Vec<(usize, NotePhonemes)> = Vec::new();
    let mut lead = Vec::new();
    let mut next_note = 0;
    for (i, symbol) in symbols.iter().enumerate() {
        if starts[i] && next_note < notes.len() {
            let mut np = NotePhonemes::default();
            if next_note == 0 {
                np.lead = std::mem::take(&mut lead);
            }
            out.push((notes[next_note], np));
            next_note += 1;
        }
        match out.last_mut() {
            Some((_, np)) => np.body.push(Ph::new(symbol.clone())),
            None => lead.push(Ph::new(symbol.clone())),
        }
    }
    if out.is_empty() && !notes.is_empty() {
        out.push((
            notes[0],
            NotePhonemes {
                body: lead,
                ..Default::default()
            },
        ));
    }
    out
}

/// How a word in a script of its own reads, in the context of its run of
/// notes (`fontelle_lang`).
#[derive(Debug, Clone, PartialEq)]
pub enum Script {
    /// Chinese characters, as pinyin syllables.
    Pinyin(Vec<String>),
    /// Hangul, as its syllables' sounds.
    Hangul(Vec<fontelle_lang::hangul::Syllable>),
}

impl Script {
    /// A word on its own.
    pub fn of(text: &str) -> Option<Script> {
        if fontelle_lang::pinyin::has_hanzi(text) {
            fontelle_lang::pinyin::read(text).map(Script::Pinyin)
        } else if fontelle_lang::hangul::has_hangul(text) {
            fontelle_lang::hangul::read(text).map(Script::Hangul)
        } else {
            None
        }
    }

    /// Each word of a phrase, read with the words sung next to it: a run is
    /// broken by a rest, a slur or a word in another script.
    pub fn of_run(notes: &[PhraseNote], words: &[Word]) -> Vec<Option<Script>> {
        // The words' texts with a break ("") wherever a rest falls between
        // two words, and where each word's text is in that list.
        let mut texts: Vec<&str> = Vec::new();
        let mut at: Vec<Option<usize>> = Vec::new();
        let mut last_end: Option<f64> = None;
        for word in words {
            let first = word.notes.first().and_then(|&n| notes.get(n));
            if let (Some(end), Some(first)) = (last_end, first)
                && first.start - end > crate::lyrics::TOUCH
            {
                texts.push("");
            }
            last_end = word
                .notes
                .last()
                .and_then(|&n| notes.get(n))
                .map(|n| n.end());
            match &word.reading {
                Reading::Text(t) => {
                    at.push(Some(texts.len()));
                    texts.push(t.trim());
                }
                _ => {
                    at.push(None);
                    texts.push("");
                }
            }
        }
        let pinyin = fontelle_lang::pinyin::read_run(&texts);
        let hangul = fontelle_lang::hangul::read_run(&texts);
        at.iter()
            .map(|i| {
                let i = (*i)?;
                if fontelle_lang::pinyin::has_hanzi(texts[i]) {
                    pinyin[i].clone().map(Script::Pinyin)
                } else {
                    hangul[i].clone().map(Script::Hangul)
                }
            })
            .collect()
    }
}

/// Stripped from a lyric's ends before a second look in the dictionary.
const PUNCTUATION: &str = ".,!?;:\"'()[]{}…“”‘’«»、。！？，";
