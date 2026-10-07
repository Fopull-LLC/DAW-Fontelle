//! Real voicebanks, when there are some: `FONTELLE_TEST_VOICEBANKS` names a
//! folder of installed banks (never in git). Each is sung a short phrase in
//! its own language; the take must be healthy and on the notes' pitch.
//!
//! `FONTELLE_TEST_VOICEBANKS=/path cargo test -p fontelle-utau --release
//! --test real_banks -- --ignored --nocapture`

mod common;

use std::collections::BTreeMap;
use std::time::Instant;

use common::{cents, measure_f0, midi_hz, rms};
use fontelle_sing::{Backend, Phrase, PhraseNote, Quality};
use fontelle_utau::UtauBackend;

#[test]
#[ignore]
fn every_installed_bank_sings_in_tune() {
    let Ok(root) = std::env::var("FONTELLE_TEST_VOICEBANKS") else {
        eprintln!("FONTELLE_TEST_VOICEBANKS not set");
        return;
    };
    let cache = common::temp_dir("real_banks_cache");
    let backend = UtauBackend::new(vec![root.into()], Some(cache));
    let voices = backend.voices();
    assert!(!voices.is_empty());
    let out = common::temp_dir("real_banks_out");
    for v in voices {
        let en = v.languages.first().map(String::as_str) == Some("en");
        let lyrics: [&str; 6] = if en {
            ["hel-", "lo", "beau-", "ti-", "ful", "world"]
        } else {
            ["さ", "く", "ら", "-", "か", "な"]
        };
        let pitches = [62.0, 64.0, 66.0, 64.0, 62.0, 59.0];
        let notes: Vec<PhraseNote> = lyrics
            .iter()
            .zip(pitches)
            .enumerate()
            .map(|(i, (l, p))| PhraseNote::new(i as f64 * 0.8, 0.8, p, *l))
            .collect();
        let phrase = Phrase {
            voice: v.id.clone(),
            language: v.languages[0].clone(),
            notes: notes.clone(),
            curves: BTreeMap::new(),
            extra_curves: BTreeMap::new(),
            settings: BTreeMap::new(),
            state: None,
            start: 0.0,
            note_ids: Vec::new(),
        };
        let mut session = backend.session().unwrap();
        for q in [Quality::Preview, Quality::Final, Quality::Final] {
            let t = Instant::now();
            let s = session.sing(&phrase, q, 1, &|| false).unwrap();
            let took = t.elapsed();
            assert!(s.samples.iter().all(|x| x.is_finite()));
            let peak = s.samples.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            let mut errs = Vec::new();
            for n in &notes {
                let f = measure_f0(
                    &s.samples,
                    s.sample_rate,
                    n.start + 0.25 - s.start,
                    n.end() - 0.1 - s.start,
                );
                errs.push(if f > 0.0 {
                    cents(f, midi_hz(n.pitch))
                } else {
                    f32::NAN
                });
            }
            let sung: Vec<String> = s
                .phonemes
                .iter()
                .map(|n| {
                    n.phonemes
                        .iter()
                        .map(|p| p.symbol.as_str())
                        .collect::<Vec<_>>()
                        .join("+")
                })
                .collect();
            eprintln!(
                "{} {q:?}: {took:?}, {:.1}s, rms {:.3}, peak {peak:.2}, cents {errs:.0?}, sung {sung:?}",
                v.name,
                s.samples.len() as f64 / s.sample_rate as f64,
                rms(&s.samples)
            );
            assert!(rms(&s.samples) > 0.01, "{} is silent", v.name);
            assert!(peak <= 1.0);
            let good = errs.iter().filter(|e| e.abs() < 25.0).count();
            assert!(good >= notes.len() - 1, "{}: {errs:?}", v.name);
            let name: String =
                v.id.chars()
                    .map(|c| if c.is_alphanumeric() { c } else { '_' })
                    .collect();
            fontelle_utau::wav::write_wav(
                &out.join(format!("{name}_{q:?}.wav")),
                &s.samples,
                s.sample_rate,
            )
            .unwrap();
        }
    }
    eprintln!("takes in {}", out.display());
}
