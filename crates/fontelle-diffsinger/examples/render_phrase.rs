//! Sings a phrase with a DiffSinger voicebank and writes it to a wav, for
//! listening.
//!
//! ```text
//! cargo run --release -p fontelle-diffsinger --example render_phrase -- \
//!     <voicebanks folder> <voice id> <language> "<lyrics>" "<midi notes>" <out.wav> \
//!     [--preview] [--seed N] [--note-length SECONDS] [--notes-pitch]
//! ```
//!
//! Lyrics and notes are space separated, one per note (`-` slurs,
//! `[phonemes]` with underscores for spaces, `a-` joins). Without a voice
//! id (`-`) the first voicebank found sings.

use std::collections::BTreeMap;
use std::time::Instant;

use fontelle_diffsinger::{DiffSinger, Options};
use fontelle_sing::{Backend, Phrase, PhraseNote, Quality};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let positional: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if positional.len() < 6 {
        eprintln!(
            "usage: render_phrase <voicebanks> <voice|-> <language> \"<lyrics>\" \"<midi notes>\" <out.wav> [--preview] [--seed=N] [--note-length=S] [--notes-pitch]"
        );
        std::process::exit(2);
    }
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        args.iter()
            .find_map(|a| a.strip_prefix(&format!("{name}=")).map(str::to_string))
    };
    let singer = DiffSinger::new(Options::new(vec![positional[0].into()]));
    for (dir, why) in singer.failed() {
        eprintln!("not a usable voicebank: {}: {why}", dir.display());
    }
    let voices = singer.voices();
    let voice = if positional[1] == "-" {
        voices.first().expect("no voicebank found").id.clone()
    } else {
        positional[1].clone()
    };
    for p in singer.problems(&voice) {
        eprintln!("{voice}: {p}");
    }
    let lyrics: Vec<String> = positional[3]
        .split_whitespace()
        .map(|s| s.replace('_', " "))
        .collect();
    let pitches: Vec<f32> = positional[4]
        .split_whitespace()
        .map(|s| s.parse().expect("a MIDI note number"))
        .collect();
    let length: f64 = value("--note-length").map_or(0.5, |v| v.parse().unwrap());
    let notes = lyrics
        .iter()
        .enumerate()
        .map(|(i, l)| {
            PhraseNote::new(
                i as f64 * length,
                length,
                pitches[i % pitches.len()],
                l.clone(),
            )
        })
        .collect();
    let mut settings = BTreeMap::new();
    if flag("--notes-pitch") {
        settings.insert("natural_pitch".to_string(), 0.0);
    }
    let phrase = Phrase {
        voice: voice.clone(),
        language: positional[2].clone(),
        notes,
        curves: BTreeMap::new(),
        extra_curves: BTreeMap::new(),
        settings,
        state: None,
        start: 0.0,
        note_ids: Vec::new(),
    };
    let quality = if flag("--preview") {
        Quality::Preview
    } else {
        Quality::Final
    };
    let seed = value("--seed").map_or(1, |v| v.parse().unwrap());
    let t = Instant::now();
    singer.preload(&voice).expect("the voicebank's models");
    eprintln!("models opened in {:.2} s", t.elapsed().as_secs_f64());
    let mut session = singer.session().unwrap();
    let t = Instant::now();
    let sung = session
        .sing(&phrase, quality, seed, &|| false)
        .expect("sung");
    let took = t.elapsed().as_secs_f64();
    let seconds = sung.samples.len() as f64 / f64::from(sung.sample_rate);
    eprintln!(
        "{voice}: {seconds:.2} s of audio in {took:.2} s ({:.2}x real time), {:?}",
        seconds / took,
        quality
    );
    for n in &sung.phonemes {
        let p: Vec<String> = n
            .phonemes
            .iter()
            .map(|p| format!("{}@{:+.3}", p.symbol, p.offset.unwrap_or(0.0)))
            .collect();
        eprintln!(
            "  note {} \"{}\": {}{}",
            n.note,
            phrase.notes[n.note].lyric,
            p.join(" "),
            if n.guessed { " (guessed)" } else { "" }
        );
    }
    write_wav(positional[5], sung.sample_rate, &sung.samples);
}

fn write_wav(path: &str, rate: u32, samples: &[f32]) {
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    let data = (samples.len() * 2) as u32;
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, out).expect("the wav written");
}
