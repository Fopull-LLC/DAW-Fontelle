//! A voicebank on disk: its character, its terms, its otos.
//!
//! A bank is a folder with `character.txt` (UTAU) or `character.yaml`
//! (OpenUtau) and an `oto.ini` in it or in its subfolders — every one of
//! them is read, as UTAU does, so a library with single-sound, VCV and
//! extra folders is one voice. Multi-pitch banks name their pitches by
//! suffix (`prefix.map`, or `character.yaml`'s subbanks); voice colours
//! ("appends") are subbanks with a colour name, or found by their shape:
//! a suffix most aliases come with (`か強`), or subfolders recorded with
//! the same aliases.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use yaml_rust2::{Yaml, YamlLoader};

use crate::oto::{self, OtoEntry};
use crate::text::{self, TextEncoding};

/// A set of aliases for a pitch range and a colour: `character.yaml`'s
/// subbanks, or one made from `prefix.map`.
#[derive(Debug, Clone, PartialEq)]
pub struct Subbank {
    /// The voice colour it belongs to, `""` for the default.
    pub colour: String,
    pub prefix: String,
    pub suffix: String,
    /// Inclusive MIDI ranges; empty means every note.
    pub tones: Vec<(u8, u8)>,
}

impl Subbank {
    fn covers(&self, tone: u8) -> bool {
        self.tones.is_empty() || self.tones.iter().any(|&(a, b)| (a..=b).contains(&tone))
    }
}

/// A voice colour (an "append"): offered per note.
#[derive(Debug, Clone, PartialEq)]
pub struct Colour {
    /// What a note's option holds.
    pub id: String,
    pub name: String,
    /// Added after the alias (before any pitch suffix), for suffix appends.
    pub suffix: String,
    /// For appends recorded as subfolders with the same aliases.
    pub folder: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Voicebank {
    pub dir: PathBuf,
    pub name: String,
    pub author: Option<String>,
    pub web: Option<String>,
    pub image: Option<PathBuf>,
    pub sample: Option<PathBuf>,
    /// `readme.txt` (and any terms file beside it), as the author wrote it.
    pub terms: Option<String>,
    pub encoding: TextEncoding,
    /// `character.yaml`'s `default_phonemizer`, as written.
    pub default_phonemizer: Option<String>,
    pub entries: Vec<OtoEntry>,
    pub subbanks: Vec<Subbank>,
    pub colours: Vec<Colour>,
    index: HashMap<String, Vec<usize>>,
}

/// Files whose names say they hold terms of use, read after `readme.txt`.
const TERMS_HINTS: [&str; 8] = [
    "規約",
    "使用許諾",
    "利用",
    "terms",
    "license",
    "licence",
    "rules",
    "tos",
];

impl Voicebank {
    /// Reads the bank in `dir`.
    pub fn load(dir: &Path) -> Result<Voicebank, String> {
        if !dir.is_dir() {
            return Err(format!("{} is not a folder", dir.display()));
        }
        let mut name = None;
        let mut author = None;
        let mut web = None;
        let mut image = None;
        let mut sample = None;
        let mut hint = None;
        let mut default_phonemizer = None;
        let mut subbanks = Vec::new();
        let mut yaml_colours = false;

        if let Ok(bytes) = std::fs::read(dir.join("character.yaml")) {
            let (s, _) = text::decode(&bytes, Some(TextEncoding::Utf8));
            if let Ok(docs) = YamlLoader::load_from_str(&s)
                && let Some(doc) = docs.first()
            {
                let get = |k: &str| {
                    doc[k]
                        .as_str()
                        .map(str::to_string)
                        .filter(|v| !v.is_empty())
                };
                name = get("name");
                author = get("author");
                web = get("web");
                image = get("image")
                    .or_else(|| get("portrait"))
                    .map(|p| dir.join(p));
                sample = get("sample").map(|p| dir.join(p));
                hint = get("text_file_encoding").and_then(|e| text::encoding_from_name(&e));
                default_phonemizer = get("default_phonemizer");
                if let Some(list) = doc["subbanks"].as_vec() {
                    for sb in list {
                        let s = |k: &str| sb[k].as_str().unwrap_or("").to_string();
                        let tones = match &sb["tone_ranges"] {
                            Yaml::Array(a) => a
                                .iter()
                                .filter_map(|r| r.as_str())
                                .filter_map(tone_range)
                                .collect(),
                            Yaml::String(r) => tone_range(r).into_iter().collect(),
                            _ => Vec::new(),
                        };
                        let colour = s("color");
                        yaml_colours |= !colour.is_empty();
                        subbanks.push(Subbank {
                            colour,
                            prefix: s("prefix"),
                            suffix: s("suffix"),
                            tones,
                        });
                    }
                }
            }
        }

        let mut encoding = hint;
        if let Ok(bytes) = std::fs::read(dir.join("character.txt")) {
            let (s, enc) = text::decode(&bytes, hint);
            encoding.get_or_insert(enc);
            for line in s.lines() {
                let Some((k, v)) = line.split_once(['=', ':']) else {
                    continue;
                };
                let v = v.trim().to_string();
                if v.is_empty() {
                    continue;
                }
                match k.trim().to_ascii_lowercase().as_str() {
                    "name" => {
                        name.get_or_insert(v);
                    }
                    "image" => {
                        image.get_or_insert(dir.join(v));
                    }
                    "sample" => {
                        sample.get_or_insert(dir.join(v));
                    }
                    "author" | "created by" | "creator" => {
                        author.get_or_insert(v);
                    }
                    "web" | "url" => {
                        web.get_or_insert(v);
                    }
                    _ => {}
                }
            }
        }

        if subbanks.is_empty()
            && let Ok(bytes) = std::fs::read(dir.join("prefix.map"))
        {
            let (s, _) = text::decode(&bytes, hint);
            subbanks = subbanks_from_prefix_map(&oto::parse_prefix_map(&s));
        }

        // Every oto.ini, the root's first, then the folders with the most
        // entries (a library's main recording before its extras).
        let mut otos = Vec::new();
        find_otos(dir, 0, &mut otos);
        let mut groups: Vec<(String, Vec<OtoEntry>)> = Vec::new();
        for path in otos {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let (s, enc) = text::decode(&bytes, hint);
            encoding.get_or_insert(enc);
            let folder_dir = path.parent().unwrap_or(dir);
            let folder = folder_dir
                .strip_prefix(dir)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            groups.push((folder.clone(), oto::parse_oto(&s, folder_dir, &folder)));
        }
        if groups.is_empty() {
            return Err(format!("no oto.ini in {}", dir.display()));
        }
        groups.sort_by(|a, b| {
            (!a.0.is_empty())
                .cmp(&!b.0.is_empty())
                .then(b.1.len().cmp(&a.1.len()))
                .then(a.0.cmp(&b.0))
        });
        let entries: Vec<OtoEntry> = groups.into_iter().flat_map(|g| g.1).collect();
        // The readme is read in the encoding the rest of the bank is in: a
        // readme of kanji alone is valid in both double-byte encodings.
        let terms = read_terms(dir, hint.or(encoding));
        let entries = resolve_files(entries);

        let name = name.unwrap_or_else(|| {
            dir.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Voicebank".into())
        });
        let mut bank = Voicebank::from_parts(dir, &name, entries, subbanks);
        if yaml_colours {
            // The bank said what its colours are; trust it over guessing.
            let mut seen = HashSet::new();
            bank.colours = bank
                .subbanks
                .iter()
                .filter(|s| !s.colour.is_empty() && seen.insert(s.colour.clone()))
                .map(|s| Colour {
                    id: s.colour.clone(),
                    name: s.colour.clone(),
                    suffix: String::new(),
                    folder: None,
                })
                .collect();
        }
        bank.author = author;
        bank.web = web;
        bank.image = image;
        bank.sample = sample;
        bank.terms = terms;
        bank.encoding = encoding.unwrap_or(TextEncoding::ShiftJis);
        bank.default_phonemizer = default_phonemizer;
        Ok(bank)
    }

    /// A bank from entries already parsed (tests, tools); colours are
    /// detected as [`Voicebank::load`] does.
    pub fn from_parts(
        dir: &Path,
        name: &str,
        entries: Vec<OtoEntry>,
        subbanks: Vec<Subbank>,
    ) -> Voicebank {
        let mut index: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            index.entry(e.alias.clone()).or_default().push(i);
        }
        let mut bank = Voicebank {
            dir: dir.to_path_buf(),
            name: name.to_string(),
            author: None,
            web: None,
            image: None,
            sample: None,
            terms: None,
            encoding: TextEncoding::Utf8,
            default_phonemizer: None,
            entries,
            subbanks,
            colours: Vec::new(),
            index,
        };
        bank.colours = bank.detect_colours();
        bank
    }

    fn entry_in(&self, alias: &str, folder: Option<&str>) -> Option<&OtoEntry> {
        let list = self.index.get(alias)?;
        match folder {
            Some(f) => list
                .iter()
                .map(|&i| &self.entries[i])
                .find(|e| e.folder == f),
            None => list.first().map(|&i| &self.entries[i]),
        }
    }

    /// The entry singing `alias` on MIDI note `tone` in `colour` (a
    /// [`Colour::id`], `""` the default): the subbank's prefix and suffix for
    /// that tone and colour first, then the plain alias.
    pub fn find(&self, alias: &str, tone: u8, colour: &str) -> Option<&OtoEntry> {
        let col = if colour.is_empty() {
            None
        } else {
            self.colours.iter().find(|c| c.id == colour)
        };
        let folder = col.and_then(|c| c.folder.as_deref());
        let colour_suffix = col.map(|c| c.suffix.as_str()).unwrap_or("");
        let try_subbanks = |name: &str| -> Option<&OtoEntry> {
            self.subbanks
                .iter()
                .filter(|s| s.colour == name && s.covers(tone))
                .find_map(|s| {
                    let full = format!("{}{alias}{colour_suffix}{}", s.prefix, s.suffix);
                    self.entry_in(&full, folder)
                        .or_else(|| self.entry_in(&full, None))
                })
        };
        if !colour.is_empty()
            && let Some(e) = try_subbanks(colour)
        {
            return Some(e);
        }
        if let Some(e) = try_subbanks("") {
            return Some(e);
        }
        if !colour_suffix.is_empty()
            && let Some(e) = self.entry_in(&format!("{alias}{colour_suffix}"), None)
        {
            return Some(e);
        }
        // Without a subbank for this tone: the plain alias, else the alias in
        // any pitch (nearest range first), so a note just outside the
        // recorded ranges still sings.
        self.entry_in(alias, folder)
            .or_else(|| self.entry_in(alias, None))
            .or_else(|| {
                let mut subs: Vec<&Subbank> = self
                    .subbanks
                    .iter()
                    .filter(|s| s.colour.is_empty() || s.colour == colour)
                    .collect();
                subs.sort_by_key(|s| {
                    s.tones
                        .iter()
                        .map(|&(a, b)| {
                            if tone < a {
                                a - tone
                            } else {
                                tone.saturating_sub(b)
                            }
                        })
                        .min()
                        .unwrap_or(0)
                });
                subs.into_iter()
                    .find_map(|s| self.entry_in(&format!("{}{alias}{}", s.prefix, s.suffix), None))
            })
    }

    /// Whether `alias` exists in any form.
    pub fn has(&self, alias: &str) -> bool {
        self.index.contains_key(alias)
            || self.subbanks.iter().any(|s| {
                self.index
                    .contains_key(&format!("{}{alias}{}", s.prefix, s.suffix))
            })
    }

    /// `alias` with any subbank prefix and suffix, then any colour suffix,
    /// taken off.
    fn base_of<'a>(&self, alias: &'a str) -> &'a str {
        let mut a = alias;
        let mut best = a;
        for s in &self.subbanks {
            if s.prefix.is_empty() && s.suffix.is_empty() {
                continue;
            }
            if let Some(x) = a.strip_prefix(s.prefix.as_str())
                && let Some(x) = x.strip_suffix(s.suffix.as_str())
                && x.len() < best.len()
                && !x.is_empty()
            {
                best = x;
            }
        }
        a = best;
        for c in &self.colours {
            if !c.suffix.is_empty()
                && let Some(x) = a.strip_suffix(c.suffix.as_str())
                && !x.is_empty()
            {
                return x;
            }
        }
        a
    }

    /// Every alias without subbank prefixes and suffixes or colour
    /// suffixes, sorted.
    pub fn base_aliases(&self) -> Vec<String> {
        let mut set: Vec<String> = self
            .index
            .keys()
            .map(|a| self.base_of(a).to_string())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        set.sort();
        set
    }

    /// The lowest and highest notes its subbanks cover (a guess when it
    /// says nothing).
    pub fn range(&self) -> (u8, u8) {
        let tones: Vec<(u8, u8)> = self.subbanks.iter().flat_map(|s| s.tones.clone()).collect();
        if tones.is_empty() {
            return (48, 76);
        }
        let lo = tones.iter().map(|t| t.0).min().unwrap_or(48);
        let hi = tones.iter().map(|t| t.1).max().unwrap_or(76);
        (lo, hi)
    }

    fn detect_colours(&self) -> Vec<Colour> {
        let mut colours = Vec::new();
        // Subfolders recorded with the same aliases.
        let mut by_folder: BTreeMap<&str, HashSet<&str>> = BTreeMap::new();
        let mut order: Vec<&str> = Vec::new();
        for e in &self.entries {
            if !by_folder.contains_key(e.folder.as_str()) {
                order.push(&e.folder);
            }
            by_folder.entry(&e.folder).or_default().insert(&e.alias);
        }
        let mut coloured: Vec<&str> = Vec::new();
        for (i, a) in order.iter().enumerate() {
            for b in &order[i + 1..] {
                let (sa, sb) = (&by_folder[a], &by_folder[b]);
                let shared = sa.intersection(sb).count();
                let small = sa.len().min(sb.len());
                if shared >= 5 && shared * 10 >= small * 6 {
                    for f in [*a, *b] {
                        if !coloured.contains(&f) {
                            coloured.push(f);
                        }
                    }
                }
            }
        }
        // The first (the root, or the largest) is the default voice.
        let mut folders: Vec<&str> = order
            .iter()
            .copied()
            .filter(|f| coloured.contains(f))
            .collect();
        if !folders.is_empty() {
            folders.remove(0);
        }
        for f in folders {
            colours.push(Colour {
                id: f.to_string(),
                name: f.rsplit('/').next().unwrap_or(f).to_string(),
                suffix: String::new(),
                folder: Some(f.to_string()),
            });
        }

        // A suffix most aliases come with. Pitch affixes are taken off
        // first; kana-only and digit-only endings are not colours (small
        // kana make きゃ from き; numbers mark duplicate takes).
        let strip_pitch = |a: &str| -> String {
            for s in &self.subbanks {
                if s.prefix.is_empty() && s.suffix.is_empty() {
                    continue;
                }
                if let Some(x) = a.strip_prefix(s.prefix.as_str())
                    && let Some(x) = x.strip_suffix(s.suffix.as_str())
                    && !x.is_empty()
                {
                    return x.to_string();
                }
            }
            a.to_string()
        };
        let set: HashSet<String> = self.index.keys().map(|a| strip_pitch(a)).collect();
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for a in &set {
            for (i, _) in a.char_indices().skip(1) {
                let (b, s) = a.split_at(i);
                if s.starts_with(' ') || b.ends_with(' ') || s.chars().count() > 8 {
                    continue;
                }
                let kana_or_digit = s.chars().all(|c| {
                    c.is_ascii_digit() || matches!(c as u32, 0x3040..=0x30FF) || c == 'ー'
                });
                if kana_or_digit || !set.contains(b) {
                    continue;
                }
                *counts.entry(s).or_default() += 1;
            }
        }
        let need = 10usize.max(set.len() / 5);
        let mut suffixes: Vec<(&str, usize)> = counts
            .into_iter()
            .filter(|&(_, n)| n >= need.min(set.len() / 3).max(8))
            .collect();
        suffixes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        for (s, _) in suffixes {
            colours.push(Colour {
                id: s.to_string(),
                name: s.trim_start_matches(['_', '-', ' ']).to_string(),
                suffix: s.to_string(),
                folder: None,
            });
        }
        colours
    }
}

/// "C1-B4" or "C4".
fn tone_range(r: &str) -> Option<(u8, u8)> {
    match r.split_once('-') {
        Some((a, b)) => Some((oto::note_number(a.trim())?, oto::note_number(b.trim())?)),
        None => {
            let n = oto::note_number(r.trim())?;
            Some((n, n))
        }
    }
}

/// One subbank per run of notes with the same prefix and suffix.
fn subbanks_from_prefix_map(map: &oto::PrefixMap) -> Vec<Subbank> {
    let mut subs: Vec<Subbank> = Vec::new();
    for (&note, (prefix, suffix)) in &map.map {
        if let Some(last) = subs.last_mut()
            && last.prefix == *prefix
            && last.suffix == *suffix
            && let Some(range) = last.tones.last_mut()
            && range.1 + 1 == note
        {
            range.1 = note;
            continue;
        }
        if let Some(s) = subs
            .iter_mut()
            .find(|s| s.prefix == *prefix && s.suffix == *suffix)
        {
            s.tones.push((note, note));
            continue;
        }
        subs.push(Subbank {
            colour: String::new(),
            prefix: prefix.clone(),
            suffix: suffix.clone(),
            tones: vec![(note, note)],
        });
    }
    subs
}

fn read_terms(dir: &Path, hint: Option<TextEncoding>) -> Option<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("txt")))
        .collect();
    files.sort();
    let name_of = |p: &Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    let readme: Vec<&PathBuf> = files
        .iter()
        .filter(|p| name_of(p).starts_with("readme"))
        .collect();
    let others: Vec<&PathBuf> = files
        .iter()
        .filter(|p| {
            let n = name_of(p);
            !n.starts_with("readme")
                && n != "character.txt"
                && TERMS_HINTS.iter().any(|h| n.contains(h))
        })
        .collect();
    let mut parts = Vec::new();
    for p in readme.iter().chain(others.iter()) {
        if let Ok(bytes) = std::fs::read(p) {
            let hint = if std::str::from_utf8(&bytes).is_ok() {
                None
            } else {
                hint
            };
            let (s, _) = text::decode(&bytes, hint);
            let s = s.trim_matches(['\u{feff}', '\0']).to_string();
            if !s.trim().is_empty() {
                parts.push((name_of(p), s));
            }
        }
    }
    match parts.len() {
        0 => None,
        1 => Some(parts.remove(0).1),
        _ => Some(
            parts
                .into_iter()
                .map(|(n, s)| format!("== {n} ==\n{s}"))
                .collect::<Vec<_>>()
                .join("\n\n"),
        ),
    }
}

fn find_otos(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut subdirs = Vec::new();
    for e in read.flatten() {
        let p = e.path();
        if p.is_dir() {
            subdirs.push(p);
        } else if p
            .file_name()
            .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("oto.ini"))
        {
            out.push(p);
        }
    }
    if depth < 3 {
        subdirs.sort();
        for d in subdirs {
            find_otos(&d, depth + 1, out);
        }
    }
}

/// Points each entry at the file as it is on disk: an oto written on
/// Windows may differ in case, or name a `.wav` the archive tool wrote in
/// another Unicode form.
fn resolve_files(mut entries: Vec<OtoEntry>) -> Vec<OtoEntry> {
    let mut listings: HashMap<PathBuf, HashMap<String, PathBuf>> = HashMap::new();
    for e in &mut entries {
        if e.file.is_file() {
            continue;
        }
        let Some(parent) = e.file.parent().map(Path::to_path_buf) else {
            continue;
        };
        let listing = listings.entry(parent.clone()).or_insert_with(|| {
            std::fs::read_dir(&parent)
                .map(|r| {
                    r.flatten()
                        .map(|f| (fold(&f.file_name().to_string_lossy()), f.path()))
                        .collect()
                })
                .unwrap_or_default()
        });
        if let Some(name) = e.file.file_name()
            && let Some(found) = listing.get(&fold(&name.to_string_lossy()))
        {
            e.file = found.clone();
        }
    }
    entries
}

/// Case-folded, with combining voiced marks composed (macOS writes
/// decomposed names: か + ゙ for が).
fn fold(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '\u{3099}' | '\u{309A}' => {
                if let Some(prev) = out.pop() {
                    let shift = if c == '\u{3099}' { 1 } else { 2 };
                    let composed = char::from_u32(prev as u32 + shift).unwrap_or(prev);
                    out.push(composed);
                }
            }
            _ => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// Whether `dir` looks like a voicebank: a `character.txt`/`.yaml` with
/// an `oto.ini` in or under it, an `oto.ini` of its own, or subfolders
/// with `oto.ini`s that are not banks themselves.
pub fn is_voicebank(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    let has = |d: &Path, f: &str| d.join(f).is_file();
    let character = |d: &Path| has(d, "character.txt") || has(d, "character.yaml");
    // A DiffSinger bank may carry an oto.ini for OpenUtau (Namine Ritsu's
    // does); it is the DiffSinger runtime's.
    if has(dir, "dsconfig.yaml") {
        return false;
    }
    if has(dir, "oto.ini") {
        return true;
    }
    let mut otos = Vec::new();
    find_otos(dir, 1, &mut otos);
    if otos.is_empty() {
        return false;
    }
    if character(dir) {
        return true;
    }
    // Bare: the subfolders hold otos but none is a bank of its own.
    let subdirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| {
            r.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    subdirs.iter().any(|d| has(d, "oto.ini")) && !subdirs.iter().any(|d| character(d))
}

/// The voicebanks in `root` and its subfolders, at most four levels deep.
pub fn scan(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    scan_into(root, 0, &mut out);
    out.sort();
    out
}

fn scan_into(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if is_voicebank(dir) {
        out.push(dir.to_path_buf());
        return;
    }
    if depth >= 4 {
        return;
    }
    if let Ok(read) = std::fs::read_dir(dir) {
        let mut subdirs: Vec<PathBuf> = read
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();
        for d in subdirs {
            scan_into(&d, depth + 1, out);
        }
    }
}
