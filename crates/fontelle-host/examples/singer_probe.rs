//! Lists the singer engines Fontelle would load, and has each one's first
//! singer sing a short phrase — the way to check an engine you just
//! installed, outside the studio.
//!
//! `cargo run -p fontelle-host --example singer_probe [folder...]`
//! (no folders: Fontelle's own singers folders, `FONTELLE_SINGERS` first).

use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::time::Instant;

use fontelle_host::{PhraseSpec, SingerEngines, SpecNote};
use fontelle_singer_abi::{QUALITY_FINAL, QUALITY_PREVIEW};

fn main() {
    let folders: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let folders = if folders.is_empty() {
        fontelle_host::singer_search_paths()
    } else {
        folders
    };
    println!("looking in: {folders:?}");
    let engines = SingerEngines::load(&folders);
    for failure in &engines.failures {
        println!("did not load {}: {}", failure.path.display(), failure.why);
    }
    for engine in engines.engines() {
        println!(
            "{} {} ({})",
            engine.name(),
            engine.version(),
            engine.path().display()
        );
        for singer in engine.singers() {
            println!(
                "  {} [{}] {:?} {}..{}{}",
                singer.name,
                singer.id,
                singer.languages,
                singer.lowest,
                singer.highest,
                if singer.research_only {
                    " RESEARCH ONLY"
                } else {
                    ""
                }
            );
        }
        let Some(singer) = engine.singers().first() else {
            continue;
        };
        let language = singer.languages.first().cloned().unwrap_or_default();
        let mid = f32::from(singer.lowest) + f32::from(singer.highest - singer.lowest) / 2.0;
        let words = if language.starts_with("ja") {
            ["さ", "く", "ら"]
        } else {
            ["hel-", "lo", "world"]
        };
        let spec = PhraseSpec {
            singer: singer.id.clone(),
            language,
            notes: words
                .iter()
                .enumerate()
                .map(|(i, w)| SpecNote {
                    start: i as f64 * 0.5,
                    duration: 0.5,
                    pitch: mid.round() + [0.0, 2.0, 4.0][i],
                    lyric: (*w).to_string(),
                })
                .collect(),
        };
        let started = Instant::now();
        let mut voice = match engine.instance() {
            Ok(v) => v,
            Err(e) => {
                println!("  no instance: {e}");
                continue;
            }
        };
        println!(
            "  instance in {:?}; version now {}",
            started.elapsed(),
            engine.version()
        );
        for quality in [QUALITY_PREVIEW, QUALITY_FINAL] {
            let t = Instant::now();
            match voice.render(&spec, quality, 1, &AtomicU32::new(0)) {
                Ok(sung) => {
                    let peak = sung.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                    println!(
                        "  quality {quality}: {:.2} s of audio at {} Hz from {:+.3} s, peak {peak:.2}, {} pitch points, phonemes {:?}, in {:?}",
                        sung.samples.len() as f64 / f64::from(sung.sample_rate.max(1)),
                        sung.sample_rate,
                        sung.start,
                        sung.pitch.len(),
                        sung.phonemes
                            .iter()
                            .map(|p| (p.note, p.phonemes.join(" "), p.guessed))
                            .collect::<Vec<_>>(),
                        t.elapsed()
                    );
                }
                Err(e) => println!("  quality {quality}: {e}"),
            }
        }
    }
}
