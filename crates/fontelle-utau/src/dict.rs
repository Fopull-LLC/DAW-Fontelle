//! English pronunciation: a dictionary derived from CMUdict (its notice is
//! `data/CMUDICT-LICENSE`, shipped beside it) and a letter-to-sound guess
//! for words it lacks.
//!
//! The derived file is CMUdict's first pronunciation of each word, stress
//! marks removed (ARPAsing banks do not record stress), one
//! `word<TAB>phones` line per word in byte order, gzipped: about 0.8 MB,
//! from `cmudict.dict` of cmusphinx/cmudict at commit 74790861 (2026-10-07).
//! It is decompressed on first use and searched in place. CMUdict's licence
//! (BSD-2-Clause) asks that binary redistributions carry its notice: ship
//! `data/CMUDICT-LICENSE` with the release's licences.

use std::io::Read;
use std::sync::OnceLock;

static CMUDICT_GZ: &[u8] = include_bytes!("../data/cmudict.tsv.gz");

pub struct Cmudict {
    text: String,
    lines: Vec<(u32, u32)>,
}

/// The dictionary, decompressed on first use.
pub fn cmudict() -> &'static Cmudict {
    static DICT: OnceLock<Cmudict> = OnceLock::new();
    DICT.get_or_init(|| {
        let mut text = String::new();
        let _ = flate2::read::GzDecoder::new(CMUDICT_GZ).read_to_string(&mut text);
        let mut lines = Vec::with_capacity(130_000);
        let mut start = 0usize;
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                if i > start {
                    lines.push((start as u32, i as u32));
                }
                start = i + 1;
            }
        }
        if start < text.len() {
            lines.push((start as u32, text.len() as u32));
        }
        Cmudict { text, lines }
    })
}

impl Cmudict {
    fn line(&self, i: usize) -> (&str, &str) {
        let (a, b) = self.lines[i];
        let l = &self.text[a as usize..b as usize];
        l.split_once('\t').unwrap_or((l, ""))
    }

    /// ARPAbet without stress, lowercase.
    pub fn lookup(&self, word: &str) -> Option<Vec<String>> {
        let w = normalise(word);
        if w.is_empty() {
            return None;
        }
        let i = self
            .lines
            .binary_search_by(|&(a, b)| {
                let l = &self.text[a as usize..b as usize];
                let key = l.split('\t').next().unwrap_or(l);
                key.cmp(w.as_str())
            })
            .ok()?;
        Some(self.line(i).1.split(' ').map(str::to_string).collect())
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Lowercase, curly apostrophes straightened, punctuation at the ends off.
fn normalise(word: &str) -> String {
    word.trim()
        .replace(['\u{2019}', '\u{2018}'], "'")
        .to_lowercase()
        .trim_matches(|c: char| !(c.is_alphanumeric() || c == '\''))
        .to_string()
}

/// The ARPAbet vowels (stress removed).
pub const VOWELS: [&str; 15] = [
    "aa", "ae", "ah", "ao", "aw", "ay", "eh", "er", "ey", "ih", "iy", "ow", "oy", "uh", "uw",
];

/// The ARPAbet consonants.
pub const CONSONANTS: [&str; 24] = [
    "b", "ch", "d", "dh", "f", "g", "hh", "jh", "k", "l", "m", "n", "ng", "p", "r", "s", "sh", "t",
    "th", "v", "w", "y", "z", "zh",
];

pub fn is_vowel(p: &str) -> bool {
    VOWELS.contains(&p)
}

/// A pronunciation for a word the dictionary lacks: English spelling
/// rules, roughly.
pub fn guess(word: &str) -> Vec<String> {
    let w: Vec<char> = normalise(word)
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect();
    let s: String = w.iter().collect();
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    let n = w.len();
    let at = |k: usize| -> char { w.get(k).copied().unwrap_or(' ') };
    let is_v = |c: char| "aeiou".contains(c);
    // Silent final e, lengthening the vowel before the last consonant.
    let magic_e = n >= 3 && at(n - 1) == 'e' && !is_v(at(n - 2)) && is_v(at(n - 3));
    let end = if magic_e { n - 1 } else { n };
    while i < end {
        let rest = &s[i..end];
        let take = |p: &str| rest.starts_with(p);
        let (phones, len): (&[&str], usize) = if take("tch") {
            (&["ch"], 3)
        } else if take("igh") {
            (&["ay"], 3)
        } else if take("sch") {
            (&["s", "k"], 3)
        } else if take("ch") {
            (&["ch"], 2)
        } else if take("sh") {
            (&["sh"], 2)
        } else if take("th") {
            (&["th"], 2)
        } else if take("ph") {
            (&["f"], 2)
        } else if take("ng") {
            (&["ng"], 2)
        } else if take("ck") {
            (&["k"], 2)
        } else if take("qu") {
            (&["k", "w"], 2)
        } else if take("wh") {
            (&["w"], 2)
        } else if take("gh") {
            (&["g"], 2)
        } else if take("ee") || take("ea") {
            (&["iy"], 2)
        } else if take("oo") {
            (&["uw"], 2)
        } else if take("ou") {
            (&["aw"], 2)
        } else if take("ow") {
            (&["ow"], 2)
        } else if take("ai") || take("ay") {
            (&["ey"], 2)
        } else if take("oi") || take("oy") {
            (&["oy"], 2)
        } else if take("au") || take("aw") {
            (&["ao"], 2)
        } else if take("er") || take("ir") || take("ur") {
            (&["er"], 2)
        } else if take("ar") {
            (&["aa", "r"], 2)
        } else if take("or") {
            (&["ao", "r"], 2)
        } else {
            let c = at(i);
            let next = at(i + 1);
            let long = magic_e && i == end - 2;
            let one: &[&str] = match c {
                'a' if long => &["ey"],
                'e' if long => &["iy"],
                'i' if long => &["ay"],
                'o' if long => &["ow"],
                'u' if long => &["uw"],
                'a' => &["ae"],
                'e' => &["eh"],
                'i' => &["ih"],
                'o' => &["aa"],
                'u' => &["ah"],
                'y' if i == 0 => &["y"],
                'y' => &["iy"],
                'c' if "eiy".contains(next) => &["s"],
                'c' | 'k' | 'q' => &["k"],
                'x' => &["k", "s"],
                'h' => &["hh"],
                'j' => &["jh"],
                'g' if "ei".contains(next) && i > 0 => &["jh"],
                'b' => &["b"],
                'd' => &["d"],
                'f' => &["f"],
                'g' => &["g"],
                'l' => &["l"],
                'm' => &["m"],
                'n' => &["n"],
                'p' => &["p"],
                'r' => &["r"],
                's' => &["s"],
                't' => &["t"],
                'v' => &["v"],
                'w' => &["w"],
                'z' => &["z"],
                _ => &[],
            };
            (one, 1)
        };
        for p in phones {
            // Doubled letters are one sound.
            if out.last() != Some(p) || is_vowel(p) {
                out.push(p);
            }
        }
        i += len;
    }
    if !out.iter().any(|p| is_vowel(p)) {
        out.push("ah");
    }
    out.into_iter().map(str::to_string).collect()
}

/// Consonant clusters English begins a syllable with.
const ONSETS: [&str; 46] = [
    "p l", "p r", "b l", "b r", "t r", "d r", "k l", "k r", "g l", "g r", "f l", "f r", "th r",
    "sh r", "s l", "s m", "s n", "s p", "s t", "s k", "s w", "t w", "d w", "k w", "g w", "th w",
    "p y", "b y", "k y", "g y", "m y", "f y", "v y", "hh y", "n y", "s f", "s p l", "s p r",
    "s t r", "s k r", "s k w", "s k l", "s p y", "s k y", "l y", "hh w",
];

fn legal_onset(c: &[String]) -> bool {
    match c.len() {
        0 => true,
        1 => c[0] != "ng",
        _ => ONSETS.contains(&c.join(" ").as_str()),
    }
}

/// Splits phonemes into syllables, one vowel each (consonants between two
/// vowels go to the later syllable as far as English allows an onset).
pub fn syllabify(phones: &[String]) -> Vec<Vec<String>> {
    let vowels: Vec<usize> = (0..phones.len())
        .filter(|&i| is_vowel(&phones[i]))
        .collect();
    if vowels.len() <= 1 {
        return if phones.is_empty() {
            Vec::new()
        } else {
            vec![phones.to_vec()]
        };
    }
    let mut out = Vec::new();
    let mut start = 0;
    for w in vowels.windows(2) {
        let (v, next) = (w[0], w[1]);
        let cluster = &phones[v + 1..next];
        // The longest legal onset at the cluster's end.
        let mut split = cluster.len();
        for k in 0..=cluster.len() {
            if legal_onset(&cluster[k..]) {
                split = k;
                break;
            }
        }
        let end = v + 1 + split;
        out.push(phones[start..end].to_vec());
        start = end;
    }
    out.push(phones[start..].to_vec());
    out
}
