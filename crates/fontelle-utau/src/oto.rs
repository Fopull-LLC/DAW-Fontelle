//! `oto.ini` and `prefix.map`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One line of an `oto.ini`: where in which sample an alias is, in
/// milliseconds.
#[derive(Debug, Clone, PartialEq)]
pub struct OtoEntry {
    pub alias: String,
    /// The sample, resolved against the `oto.ini`'s folder.
    pub file: PathBuf,
    /// The subfolder of the bank the `oto.ini` is in, `""` for the root.
    pub folder: String,
    /// Where the unit starts in the file.
    pub offset: f64,
    /// The fixed (never stretched) part, from `offset`.
    pub consonant: f64,
    /// Positive: cut this much from the file's end. Negative: the unit is
    /// this long from `offset`.
    pub cutoff: f64,
    /// From `offset`, the point that lands on the note's start.
    pub preutterance: f64,
    /// From `offset`, how long it fades in over the unit before it.
    pub overlap: f64,
}

impl OtoEntry {
    /// Where the unit ends, ms from the file's start, for a file
    /// `file_ms` long.
    pub fn end_ms(&self, file_ms: f64) -> f64 {
        let end = if self.cutoff < 0.0 {
            self.offset - self.cutoff
        } else {
            file_ms - self.cutoff
        };
        end.min(file_ms).max(self.offset)
    }
}

/// Parses an `oto.ini` already decoded to text. Lines that are not
/// `file=alias,numbers` are skipped, as UTAU does; missing numbers are 0.
pub fn parse_oto(text: &str, dir: &Path, folder: &str) -> Vec<OtoEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches(['\r', '\n']);
        let Some((file, rest)) = line.split_once('=') else {
            continue;
        };
        let file = file.trim();
        if file.is_empty() {
            continue;
        }
        let mut fields = rest.split(',');
        let alias = fields.next().unwrap_or("").to_string();
        let mut num = || -> f64 {
            fields
                .next()
                .and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite())
                .unwrap_or(0.0)
        };
        let (offset, consonant, cutoff, preutterance, overlap) =
            (num(), num(), num(), num(), num());
        let alias = if alias.is_empty() {
            // UTAU's rule: the file's name without its extension.
            let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
            name.strip_suffix(".wav")
                .or_else(|| name.strip_suffix(".WAV"))
                .unwrap_or(name)
                .to_string()
        } else {
            alias
        };
        let mut path = dir.to_path_buf();
        for part in file.split(['/', '\\']).filter(|p| !p.is_empty()) {
            path.push(part);
        }
        out.push(OtoEntry {
            alias,
            file: path,
            folder: folder.to_string(),
            offset,
            consonant,
            cutoff,
            preutterance,
            overlap,
        });
    }
    out
}

/// `prefix.map`: a prefix and suffix per note number.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrefixMap {
    pub map: BTreeMap<u8, (String, String)>,
}

impl PrefixMap {
    /// The (prefix, suffix) for a MIDI note.
    pub fn get(&self, note: u8) -> (&str, &str) {
        self.map
            .get(&note)
            .map(|(p, s)| (p.as_str(), s.as_str()))
            .unwrap_or(("", ""))
    }
}

pub fn parse_prefix_map(text: &str) -> PrefixMap {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        let mut parts = line.split('\t');
        let Some(note) = parts.next().and_then(|n| note_number(n.trim())) else {
            continue;
        };
        let prefix = parts.next().unwrap_or("").to_string();
        let suffix = parts.next().unwrap_or("").to_string();
        map.insert(note, (prefix, suffix));
    }
    PrefixMap { map }
}

/// "C4" → 60, "C#4" → 61, "Bb3" → 58.
pub fn note_number(name: &str) -> Option<u8> {
    let mut chars = name.chars();
    let base = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest: String = chars.collect();
    let (shift, octave) = if let Some(r) = rest.strip_prefix('#') {
        (1, r)
    } else if let Some(r) = rest.strip_prefix('b') {
        (-1, r)
    } else {
        (0, rest.as_str())
    };
    let octave: i32 = octave.parse().ok()?;
    let n = (octave + 1) * 12 + base + shift;
    u8::try_from(n).ok().filter(|&n| n < 128)
}
