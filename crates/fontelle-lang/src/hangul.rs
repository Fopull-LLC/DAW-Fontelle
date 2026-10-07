//! Korean hangul syllable blocks to their sounds.
//!
//! A block is `0xAC00 + (initial × 21 + vowel) × 28 + final` (Unicode §3.12),
//! so nothing here is a table of syllables: only the 19 initials, 21 vowels
//! and 27 finals, romanised as Korean voicebanks name their sounds (Revised
//! Romanisation's letters: `g kk n d tt r m b pp s ss j jj ch k t p h`,
//! `a ae ya yae eo e yeo ye o wa wae oe yo u wo we wi yu eu ui i`).
//!
//! Sung, a coda is one of seven sounds (`k n t l m p ng`), and two changes
//! across syllables matter enough to make: a coda carried into a following
//! vowel (연음, `음악` → `eu` `mak`; a cluster leaves its first consonant,
//! `ㅎ` goes silent) and a stop before `ㄴ`/`ㅁ` becoming a nasal (비음화,
//! `합니다` → `ham` `ni` `da`). Others (ㄹ after ㄴ, aspiration by ㅎ,
//! tensing) are left as written; a writer who wants them types the block
//! that is sung.

const FIRST: u32 = 0xAC00;
const LAST: u32 = 0xD7A3;

const ONSETS: [&str; 19] = [
    "g", "kk", "n", "d", "tt", "r", "m", "b", "pp", "s", "ss", "", "j", "jj", "ch", "k", "t", "p",
    "h",
];

const VOWELS: [&str; 21] = [
    "a", "ae", "ya", "yae", "eo", "e", "yeo", "ye", "o", "wa", "wae", "oe", "yo", "u", "wo", "we",
    "wi", "yu", "eu", "ui", "i",
];

/// Each final (1..=27; 0 is none): the coda it is sung as alone, and when a
/// vowel follows, what stays as the coda and the initial it carries over
/// (an index into [`ONSETS`], 11 being ㅇ: nothing carried).
const FINALS: [(&str, &str, u32); 28] = [
    ("", "", 11),     // none
    ("k", "", 0),     // ㄱ
    ("k", "", 1),     // ㄲ
    ("k", "k", 9),    // ㄳ
    ("n", "", 2),     // ㄴ
    ("n", "n", 12),   // ㄵ
    ("n", "", 2),     // ㄶ: ㅎ silent, ㄴ carried
    ("t", "", 3),     // ㄷ
    ("l", "", 5),     // ㄹ
    ("k", "l", 0),    // ㄺ
    ("m", "l", 6),    // ㄻ
    ("l", "l", 7),    // ㄼ
    ("l", "l", 9),    // ㄽ
    ("l", "l", 16),   // ㄾ
    ("p", "l", 17),   // ㄿ
    ("l", "", 5),     // ㅀ: ㅎ silent, ㄹ carried
    ("m", "", 6),     // ㅁ
    ("p", "", 7),     // ㅂ
    ("p", "p", 9),    // ㅄ
    ("t", "", 9),     // ㅅ
    ("t", "", 10),    // ㅆ
    ("ng", "ng", 11), // ㅇ: stays
    ("t", "", 12),    // ㅈ
    ("t", "", 14),    // ㅊ
    ("k", "", 15),    // ㅋ
    ("t", "", 16),    // ㅌ
    ("p", "", 17),    // ㅍ
    ("t", "", 11),    // ㅎ: silent before a vowel
];

/// One sung syllable: its block and its sounds, romanised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Syllable {
    /// The block as written.
    pub text: char,
    /// `g`, `kk`, `n`, … or empty (ㅇ, or nothing).
    pub onset: &'static str,
    /// `a`, `eo`, `wa`, …
    pub vowel: &'static str,
    /// The coda as sung: `k`, `n`, `t`, `l`, `m`, `p`, `ng`, or empty.
    pub coda: &'static str,
}

impl Syllable {
    /// The whole syllable romanised: `han`, `geul`.
    pub fn roman(&self) -> String {
        format!("{}{}{}", self.onset, self.vowel, self.coda)
    }

    /// Its vowel alone (`한` → `아`): what a slur after a rest sings again.
    pub fn vowel_only(&self) -> Syllable {
        let v = VOWELS.iter().position(|o| *o == self.vowel).unwrap_or(0) as u32;
        Syllable {
            text: compose(11, v, 0).unwrap_or('아'),
            onset: "",
            vowel: self.vowel,
            coda: "",
        }
    }

    /// The block without its coda, as a CV voicebank names it (`한` → `하`),
    /// with the onset as sung (`음악`'s `악` is `마`).
    pub fn open(&self) -> char {
        let i = ONSETS.iter().position(|o| *o == self.onset).unwrap_or(11) as u32;
        let v = VOWELS.iter().position(|o| *o == self.vowel).unwrap_or(0) as u32;
        compose(i, v, 0).unwrap_or(self.text)
    }
}

/// Whether `c` is a precomposed hangul syllable (가–힣).
pub fn is_syllable(c: char) -> bool {
    (FIRST..=LAST).contains(&(c as u32))
}

/// Whether `text` has a hangul syllable in it.
pub fn has_hangul(text: &str) -> bool {
    text.chars().any(is_syllable)
}

/// A block's (initial, vowel, final) indices in Unicode's jamo order
/// (final 0 is none).
pub fn decompose(c: char) -> Option<(u32, u32, u32)> {
    if !is_syllable(c) {
        return None;
    }
    let s = c as u32 - FIRST;
    Some((s / (21 * 28), (s % (21 * 28)) / 28, s % 28))
}

/// The block with these indices.
pub fn compose(initial: u32, vowel: u32, fin: u32) -> Option<char> {
    if initial >= 19 || vowel >= 21 || fin >= 28 {
        return None;
    }
    char::from_u32(FIRST + (initial * 21 + vowel) * 28 + fin)
}

/// What one lyric says, a syllable per block; `None` when it is not hangul
/// throughout.
pub fn read(text: &str) -> Option<Vec<Syllable>> {
    read_run(&[text]).pop().flatten()
}

/// Sung punctuation is not sung.
fn is_punctuation(c: char) -> bool {
    c.is_ascii_punctuation() || c.is_whitespace() || "…“”‘’「」『』、。！？，".contains(c)
}

/// What each lyric of a run of notes says, read together so a coda carries
/// into the next note's vowel (`음악` sung `음` `악` is `eu` `mak`).
pub fn read_run(lyrics: &[&str]) -> Vec<Option<Vec<Syllable>>> {
    // Every block of the run, by lyric; a lyric that is not hangul breaks
    // the run (a slur, a rest's word in another script).
    let blocks: Vec<Option<Vec<char>>> = lyrics
        .iter()
        .map(|l| {
            let t = l.trim_matches(is_punctuation);
            (!t.is_empty() && t.chars().all(is_syllable)).then(|| t.chars().collect())
        })
        .collect();
    let mut out: Vec<Option<Vec<Syllable>>> = Vec::with_capacity(lyrics.len());
    let mut i = 0;
    while i < blocks.len() {
        let Some(_) = &blocks[i] else {
            out.push(None);
            i += 1;
            continue;
        };
        // The run: consecutive hangul lyrics.
        let mut j = i;
        while j < blocks.len() && blocks[j].is_some() {
            j += 1;
        }
        let flat: Vec<char> = blocks[i..j].iter().flatten().flatten().copied().collect();
        let mut sung = sing(&flat).into_iter();
        for b in &blocks[i..j] {
            let n = b.as_ref().map_or(0, Vec::len);
            out.push(Some(sung.by_ref().take(n).collect()));
        }
        i = j;
    }
    out
}

/// A run of blocks as sung, with the sound changes across them.
fn sing(blocks: &[char]) -> Vec<Syllable> {
    let parts: Vec<(u32, u32, u32)> = blocks.iter().filter_map(|c| decompose(*c)).collect();
    let mut onsets: Vec<u32> = parts.iter().map(|p| p.0).collect();
    let mut codas: Vec<&'static str> = parts.iter().map(|p| FINALS[p.2 as usize].0).collect();
    for k in 0..parts.len().saturating_sub(1) {
        let fin = parts[k].2;
        if fin == 0 {
            continue;
        }
        let next = onsets[k + 1];
        if next == 11 {
            // A vowel follows: the coda carries over.
            let (_, stays, carried) = FINALS[fin as usize];
            if fin == 21 {
                continue;
            }
            codas[k] = stays;
            onsets[k + 1] = carried;
        } else if next == 2 || next == 6 {
            // ㄴ or ㅁ follows: a stop becomes the nasal of its place.
            codas[k] = match codas[k] {
                "k" => "ng",
                "t" => "n",
                "p" => "m",
                c => c,
            };
        }
    }
    parts
        .iter()
        .enumerate()
        .map(|(k, p)| Syllable {
            text: blocks[k],
            onset: ONSETS[onsets[k] as usize],
            vowel: VOWELS[p.1 as usize],
            coda: codas[k],
        })
        .collect()
}
