//! Sings a phrase with an UTAU voicebank to a WAV file.
//!
//! ```text
//! cargo run --release -p fontelle-utau --example utau_sing -- \
//!     <bank folder> <out.wav> "さ:62 く:64 ら:66 -:64" [seconds per note] [preview|final]
//! ```
//!
//! Each word is `lyric:midi`. Prints the bank, its detected phonemizer, what
//! each note was sung as, timings, and each note's measured pitch error.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use fontelle_sing::{Phrase, PhraseNote, Quality};
use fontelle_utau::bank::Voicebank;
use fontelle_utau::cache::{AnalysisCache, default_cache_dir};
use fontelle_utau::{phonemize, sing_with, wav};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!(
            "usage: utau_sing <bank> <out.wav> \"lyric:midi ...\" [note seconds] [preview|final]"
        );
        std::process::exit(2);
    }
    let dir = PathBuf::from(&args[1]);
    let out = PathBuf::from(&args[2]);
    let len: f64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0.6);
    let quality = match args.get(5).map(String::as_str) {
        Some("preview") => Quality::Preview,
        _ => Quality::Final,
    };
    let t = Instant::now();
    let bank = Voicebank::load(&dir).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    let kind = phonemize::detect(&bank);
    eprintln!(
        "{}: {} entries, {} colours, {:?} ({:?} to load)",
        bank.name,
        bank.entries.len(),
        bank.colours.len(),
        kind,
        t.elapsed()
    );
    let notes: Vec<PhraseNote> = args[3]
        .split_whitespace()
        .enumerate()
        .map(|(i, w)| {
            let (lyric, pitch) = w.rsplit_once(':').unwrap_or((w, "60"));
            PhraseNote::new(i as f64 * len, len, pitch.parse().unwrap_or(60.0), lyric)
        })
        .collect();
    let phrase = Phrase {
        voice: String::new(),
        language: kind.language().into(),
        notes: notes.clone(),
        curves: BTreeMap::new(),
        extra_curves: BTreeMap::new(),
        settings: BTreeMap::new(),
        state: None,
    };
    let cache_dir = std::env::var_os("FONTELLE_UTAU_CACHE")
        .map(PathBuf::from)
        .or_else(default_cache_dir);
    let cache = AnalysisCache::new(cache_dir, 512 << 20);
    for pass in ["first", "again"] {
        let t = Instant::now();
        let s = sing_with(&bank, &cache, &phrase, quality, 1, &|| false).unwrap();
        eprintln!(
            "{pass}: {:?} for {:.2} s at {} Hz",
            t.elapsed(),
            s.samples.len() as f64 / s.sample_rate as f64,
            s.sample_rate
        );
        if pass == "again" {
            for (n, sung) in notes.iter().zip(&s.phonemes) {
                let list: Vec<String> = sung
                    .phonemes
                    .iter()
                    .map(|p| format!("{}@{:+.3}", p.symbol, p.offset.unwrap_or(0.0)))
                    .collect();
                eprintln!(
                    "  {:>6} {:>5.1}  {}{}",
                    n.lyric,
                    n.pitch,
                    list.join(" "),
                    if sung.guessed { " (guessed)" } else { "" }
                );
            }
            wav::write_wav(&out, &s.samples, s.sample_rate).unwrap();
            eprintln!("wrote {}", out.display());
        }
    }
}
