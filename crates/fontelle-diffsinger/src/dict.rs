//! A bank's phoneme dictionary, `dsdict.yaml` or `dsdict-<lang>.yaml`:
//! which symbols are vowels (a syllable's nucleus, where a note begins),
//! which are glides, and how each word is said.
//!
//! ```yaml
//! symbols:
//! - symbol: en/aa
//!   type: vowel          # vowel, semivowel, liquid, or any consonant kind
//! entries:
//! - grapheme: hello
//!   phonemes: [en/hh, en/ax, en/l, en/ow]
//! replacements:          # optional
//! - {from: aa, to: en/aa}
//! ```
//!
//! English dictionaries run to tens of megabytes, so the usual block layout
//! is read by a line scanner; anything it does not recognise is handed to
//! the YAML parser instead, which reads the same thing more slowly.

use std::collections::HashMap;

/// What kind of sound a symbol is, as far as placing it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Vowel,
    /// A semivowel or liquid: in consonant–glide–vowel the glide is where
    /// the note begins.
    Glide,
    Consonant,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dictionary {
    symbols: HashMap<String, SymbolKind>,
    entries: HashMap<String, Vec<String>>,
    replacements: HashMap<String, String>,
}

impl Dictionary {
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = crate::config::strip_bom(text);
        match scan(text) {
            Some(raw) => Ok(raw.build()),
            None => Self::parse_yaml(text),
        }
    }

    /// The YAML parser alone, for files the scanner does not take (and to
    /// test that both read alike).
    pub fn parse_yaml(text: &str) -> Result<Self, String> {
        let text = crate::config::strip_bom(text);
        if text.trim().is_empty() {
            return Ok(Raw::default().build());
        }
        let y: DictYaml = serde_yaml_ng::from_str(text).map_err(|e| e.to_string())?;
        let mut raw = Raw::default();
        for s in y.symbols.unwrap_or_default() {
            raw.symbols.push((s.symbol, s.kind.unwrap_or_default()));
        }
        for e in y.entries.unwrap_or_default() {
            raw.entries
                .push((e.grapheme, e.phonemes.unwrap_or_default()));
        }
        for r in y.replacements.unwrap_or_default() {
            raw.replacements.push((r.from, r.to));
        }
        Ok(raw.build())
    }

    /// How a word is said: as written, then lower case.
    pub fn query(&self, word: &str) -> Option<&[String]> {
        if let Some(p) = self.entries.get(word) {
            return Some(p);
        }
        let lower = word.to_lowercase();
        self.entries.get(&lower).map(Vec::as_slice)
    }

    pub fn kind(&self, symbol: &str) -> Option<SymbolKind> {
        self.symbols.get(symbol).copied()
    }

    pub fn is_vowel(&self, symbol: &str) -> bool {
        self.kind(symbol) == Some(SymbolKind::Vowel)
    }

    pub fn is_glide(&self, symbol: &str) -> bool {
        self.kind(symbol) == Some(SymbolKind::Glide)
    }

    /// Each `<language>/x` symbol also declared as plain `x` where the model
    /// knows `x` and the dictionary does not: SPORE's duration dictionary
    /// names `en/w` for an acoustic model whose phonemes are `w`.
    pub fn with_unprefixed(mut self, language: &str, known: impl Fn(&str) -> bool) -> Self {
        let prefix = format!("{language}/");
        let add: Vec<(String, SymbolKind)> = self
            .symbols
            .iter()
            .filter_map(|(s, k)| {
                let bare = s.strip_prefix(&prefix)?;
                (known(bare) && !self.symbols.contains_key(bare)).then(|| (bare.to_string(), *k))
            })
            .collect();
        self.symbols.extend(add);
        self
    }

    /// Whether the dictionary declares the symbol (`SP` and `AP` always).
    pub fn has_symbol(&self, symbol: &str) -> bool {
        self.kind(symbol).is_some()
    }

    /// Every declared symbol, sorted.
    pub fn symbols(&self) -> Vec<String> {
        let mut v: Vec<String> = self.symbols.keys().cloned().collect();
        v.sort();
        v
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn replacements(&self) -> &HashMap<String, String> {
        &self.replacements
    }
}

/// What a file says, before it is checked.
#[derive(Default)]
struct Raw {
    symbols: Vec<(String, String)>,
    entries: Vec<(String, Vec<String>)>,
    replacements: Vec<(String, String)>,
}

impl Raw {
    fn build(self) -> Dictionary {
        let mut symbols = HashMap::with_capacity(self.symbols.len() + 2);
        for (symbol, kind) in self.symbols {
            let kind = match kind.trim() {
                "vowel" => SymbolKind::Vowel,
                "semivowel" | "liquid" => SymbolKind::Glide,
                _ => SymbolKind::Consonant,
            };
            symbols.insert(symbol, kind);
        }
        // Silence and breath are a syllable of their own wherever they are.
        symbols.insert("SP".into(), SymbolKind::Vowel);
        symbols.insert("AP".into(), SymbolKind::Vowel);
        let mut entries = HashMap::with_capacity(self.entries.len());
        for (grapheme, phonemes) in self.entries {
            // A symbol the file does not declare cannot be placed; it is
            // dropped from the entry, as OpenUtau does.
            let phonemes: Vec<String> = phonemes
                .into_iter()
                .filter(|p| symbols.contains_key(p))
                .collect();
            entries.insert(grapheme, phonemes);
        }
        Dictionary {
            symbols,
            entries,
            replacements: self.replacements.into_iter().collect(),
        }
    }
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct DictYaml {
    symbols: Option<Vec<SymbolYaml>>,
    entries: Option<Vec<EntryYaml>>,
    replacements: Option<Vec<ReplacementYaml>>,
}

#[derive(serde::Deserialize)]
struct SymbolYaml {
    symbol: String,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(serde::Deserialize)]
struct EntryYaml {
    grapheme: String,
    phonemes: Option<Vec<String>>,
}

#[derive(serde::Deserialize)]
struct ReplacementYaml {
    from: String,
    to: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Symbols,
    Entries,
    Replacements,
    Other,
}

/// One list item as the scanner gathers it.
#[derive(Default)]
struct Item {
    fields: Vec<(String, Field)>,
}

enum Field {
    Scalar(String),
    List(Vec<String>),
}

impl Item {
    fn scalar(&self, key: &str) -> Option<String> {
        self.fields.iter().rev().find_map(|(k, f)| match f {
            Field::Scalar(s) if k == key => Some(s.clone()),
            _ => None,
        })
    }

    fn list(&mut self, key: &str) -> Option<Vec<String>> {
        let i = self.fields.iter().rposition(|(k, _)| k == key)?;
        match std::mem::replace(&mut self.fields[i].1, Field::List(Vec::new())) {
            Field::List(l) => Some(l),
            // `phonemes:` with nothing after it and no items: empty.
            Field::Scalar(s) if s.is_empty() => Some(Vec::new()),
            Field::Scalar(_) => None,
        }
    }
}

/// The block layout every exporter writes, read line by line. `None` for
/// anything else, which the YAML parser then reads.
fn scan(text: &str) -> Option<Raw> {
    let mut raw = Raw::default();
    let mut section = Section::Other;
    let mut item: Option<(usize, Item)> = None;
    let flush = |raw: &mut Raw, section: Section, item: Option<(usize, Item)>| -> Option<()> {
        let Some((_, mut item)) = item else {
            return Some(());
        };
        match section {
            Section::Symbols => {
                let symbol = item.scalar("symbol")?;
                raw.symbols
                    .push((symbol, item.scalar("type").unwrap_or_default()));
            }
            Section::Entries => {
                let grapheme = item.scalar("grapheme")?;
                let phonemes = item.list("phonemes").unwrap_or_default();
                raw.entries.push((grapheme, phonemes));
            }
            Section::Replacements => {
                raw.replacements
                    .push((item.scalar("from")?, item.scalar("to")?));
            }
            Section::Other => {}
        }
        Some(())
    };
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        let content = line.trim_start();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        if content.starts_with("---") || content.starts_with("...") || content.starts_with('%') {
            return None;
        }
        let indent = line.len() - content.len();
        if line.starts_with('\t') {
            return None;
        }
        if indent == 0 && !content.starts_with('-') {
            // A top-level key.
            flush(&mut raw, section, item.take())?;
            let (key, value) = split_key(content)?;
            section = match key.as_str() {
                "symbols" => Section::Symbols,
                "entries" => Section::Entries,
                "replacements" => Section::Replacements,
                _ => Section::Other,
            };
            let value = value.trim();
            if !(value.is_empty() || value == "[]" || section == Section::Other) {
                return None;
            }
            continue;
        }
        if section == Section::Other {
            // Something under a key the dictionary does not use.
            if item.is_none() && indent > 0 {
                continue;
            }
            if content.starts_with('-') {
                continue;
            }
        }
        if let Some(rest) =
            content
                .strip_prefix("- ")
                .or(if content == "-" { Some("") } else { None })
        {
            let starts_item = match &item {
                None => true,
                Some((item_indent, _)) => indent == *item_indent,
            };
            if starts_item {
                flush(&mut raw, section, item.take())?;
                let rest = rest.trim();
                if rest.starts_with('{') || rest.starts_with('[') || rest.starts_with('-') {
                    return None;
                }
                let mut it = Item::default();
                if !rest.is_empty() {
                    let (key, value) = split_key(rest)?;
                    push_field(&mut it, key, value)?;
                }
                item = Some((indent, it));
            } else {
                // An element of the item's last (block) sequence.
                let (_, it) = item.as_mut()?;
                let (_, field) = it.fields.last_mut()?;
                let element = scalar(rest.trim())?;
                match field {
                    Field::List(l) => l.push(element),
                    Field::Scalar(s) if s.is_empty() => *field = Field::List(vec![element]),
                    Field::Scalar(_) => return None,
                }
            }
            continue;
        }
        // A key of the current item.
        let (item_indent, it) = item.as_mut()?;
        if indent <= *item_indent {
            return None;
        }
        let (key, value) = split_key(content)?;
        push_field(it, key, value)?;
    }
    flush(&mut raw, section, item.take())?;
    Some(raw)
}

fn push_field(item: &mut Item, key: String, value: &str) -> Option<()> {
    let value = value.trim();
    let field = if let Some(inner) = value.strip_prefix('[') {
        let inner = inner.strip_suffix(']')?;
        if inner.contains(['[', '{', '"', '\'']) {
            return None;
        }
        Field::List(
            inner
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(scalar)
                .collect::<Option<Vec<_>>>()?,
        )
    } else {
        Field::Scalar(scalar(value)?)
    };
    item.fields.push((key, field));
    Some(())
}

/// `key: value` (the value may be empty).
fn split_key(s: &str) -> Option<(String, &str)> {
    if s.starts_with(['"', '\'', '{', '[', '?', '&', '*', '!', '|', '>']) {
        return None;
    }
    let colon = s
        .find(": ")
        .or_else(|| s.strip_suffix(':').map(|k| k.len()))?;
    let key = s[..colon].trim();
    if key.is_empty() {
        return None;
    }
    let value = s.get(colon + 1..).unwrap_or("");
    Some((key.to_string(), value))
}

/// A plain, single- or double-quoted scalar. `None` for the forms only the
/// YAML parser reads (anchors, tags, block scalars, escapes beyond the
/// simple ones).
fn scalar(s: &str) -> Option<String> {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix('\'') {
        let inner = inner.strip_suffix('\'')?;
        return Some(inner.replace("''", "'"));
    }
    if let Some(inner) = s.strip_prefix('"') {
        let inner = inner.strip_suffix('"')?;
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                _ => return None,
            }
        }
        return Some(out);
    }
    if s.starts_with(['&', '*', '!', '|', '>', '{', '[', '@', '`', '%']) {
        return None;
    }
    // A comment after a plain scalar.
    let s = match s.find(" #") {
        Some(i) => s[..i].trim_end(),
        None => s,
    };
    if matches!(s, "~" | "null" | "Null" | "NULL") {
        return None;
    }
    Some(s.to_string())
}
