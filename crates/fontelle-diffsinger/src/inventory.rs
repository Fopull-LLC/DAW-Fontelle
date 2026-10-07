//! A model's phoneme inventory (`phonemes.txt` or `phonemes.json`) and its
//! language ids (`languages.json`).

use std::collections::HashMap;

/// Phoneme symbol to the token id the model was trained with. Several
/// symbols may share an id (merged phonemes of a multi-language bank).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inventory {
    tokens: HashMap<String, i64>,
    /// Symbols in the order the file lists them.
    order: Vec<String>,
}

impl Inventory {
    /// `.json` (an object of symbol to id) when `json`, else text: one
    /// symbol a line, its line number the id.
    pub fn parse(text: &str, json: bool) -> Result<Self, String> {
        let text = crate::config::strip_bom(text);
        let mut inv = Inventory::default();
        if json {
            let map: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(text).map_err(|e| e.to_string())?;
            let mut pairs = Vec::with_capacity(map.len());
            for (symbol, id) in map {
                let id = id
                    .as_i64()
                    .ok_or_else(|| format!("the id of \"{symbol}\" is not a whole number"))?;
                pairs.push((id, symbol));
            }
            // File order is not kept by the JSON reader; id order is the
            // inventory's own order, which is as good for listing.
            pairs.sort();
            for (id, symbol) in pairs {
                inv.tokens.insert(symbol.clone(), id);
                inv.order.push(symbol);
            }
        } else {
            for (i, line) in text.lines().enumerate() {
                let symbol = line.trim_end_matches('\r');
                if symbol.trim().is_empty() {
                    continue;
                }
                inv.tokens.insert(symbol.to_string(), i as i64);
                inv.order.push(symbol.to_string());
            }
        }
        Ok(inv)
    }

    pub fn token(&self, symbol: &str) -> Option<i64> {
        self.tokens.get(symbol).copied()
    }

    pub fn contains(&self, symbol: &str) -> bool {
        self.token(symbol).is_some()
    }

    /// Every symbol, in file order.
    pub fn symbols(&self) -> &[String] {
        &self.order
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// Language code to id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Languages {
    ids: HashMap<String, i64>,
}

impl Languages {
    pub fn parse(json: &str) -> Result<Self, String> {
        let ids: std::collections::HashMap<String, i64> =
            serde_json::from_str(crate::config::strip_bom(json)).map_err(|e| e.to_string())?;
        Ok(Self { ids })
    }

    /// The id of the language a symbol belongs to (`en/hh` is `en`'s);
    /// a symbol with no language (`SP`, `AP`) or of a language the model
    /// does not have is 0.
    pub fn id_of_symbol(&self, symbol: &str) -> i64 {
        symbol_language(symbol)
            .and_then(|code| self.ids.get(code).copied())
            .unwrap_or(0)
    }

    /// The codes, by id.
    pub fn codes(&self) -> Vec<String> {
        let mut codes: Vec<(&i64, &String)> = self.ids.iter().map(|(c, i)| (i, c)).collect();
        codes.sort();
        codes.into_iter().map(|(_, c)| c.clone()).collect()
    }

    pub fn contains(&self, code: &str) -> bool {
        self.ids.contains_key(code)
    }
}

/// The language prefix of a symbol (`en` of `en/hh`), if it has one.
pub fn symbol_language(symbol: &str) -> Option<&str> {
    symbol.split_once('/').map(|(lang, _)| lang)
}
