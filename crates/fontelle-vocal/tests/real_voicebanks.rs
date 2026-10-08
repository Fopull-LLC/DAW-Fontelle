//! Every voicebank in `FONTELLE_TEST_VOICEBANKS` sings through the registry's
//! built-in runtimes, the way a vocal track reaches them. Ignored by default:
//! third-party voicebanks are never in the repository.
//!
//! `FONTELLE_TEST_VOICEBANKS=/path/to/voicebanks cargo test --release -p
//! fontelle-vocal --test real_voicebanks -- --ignored --nocapture`

use std::path::PathBuf;

use fontelle_sing::{Phrase, PhraseNote, Quality};

#[test]
#[ignore = "needs FONTELLE_TEST_VOICEBANKS"]
fn every_installed_voicebank_sings_through_the_registry() {
    let Some(dir) = std::env::var_os("FONTELLE_TEST_VOICEBANKS").map(PathBuf::from) else {
        eprintln!("FONTELLE_TEST_VOICEBANKS not set");
        return;
    };
    let mut sung = 0;
    for builtin in fontelle_vocal::builtin_backends() {
        let backend = (builtin.make)(&dir);
        for voice in backend.voices() {
            let language = voice
                .languages
                .first()
                .cloned()
                .unwrap_or_else(|| "ja".into());
            let lyrics: [&str; 3] = match language.as_str() {
                "en" => ["hel-", "lo", "world"],
                "zh" => ["ni", "hao", "a"],
                _ => ["さ", "く", "ら"],
            };
            let mid = f32::from(voice.lowest.max(48).midpoint(voice.highest.min(76)));
            let notes: Vec<PhraseNote> = lyrics
                .iter()
                .enumerate()
                .map(|(i, l)| PhraseNote::new(i as f64 * 0.5, 0.5, mid + [0.0, 2.0, 4.0][i], *l))
                .collect();
            let phrase = Phrase::new(voice.id.clone(), language.clone(), notes);
            let mut session = backend.session().expect("a session");
            let take = session
                .sing(&phrase, Quality::Preview, 1, &|| false)
                .unwrap_or_else(|e| panic!("{} / {}: {e}", builtin.key, voice.id));
            let peak = take.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
            assert!(take.samples.iter().all(|s| s.is_finite()), "{}", voice.id);
            assert!(peak > 0.01 && peak <= 1.0, "{}: peak {peak}", voice.id);
            println!(
                "{:22} {:40} {language}: {:.2} s, peak {peak:.2}, {:?}",
                builtin.key,
                voice.id,
                take.samples.len() as f64 / f64::from(take.sample_rate),
                take.phonemes
                    .iter()
                    .map(|n| n
                        .phonemes
                        .iter()
                        .map(|p| p.symbol.as_str())
                        .collect::<Vec<_>>()
                        .join(" "))
                    .collect::<Vec<_>>()
            );
            sung += 1;
        }
    }
    assert!(sung > 0, "no voicebank found in {}", dir.display());
}

/// Every real bank's voice is found among the installed banks, and its
/// picture there is the one the runtime shows — the picker's card and the
/// Installed page's row show one face.
#[test]
#[ignore = "needs FONTELLE_TEST_VOICEBANKS"]
fn every_real_voice_is_one_installed_bank_with_one_face() {
    let Some(dir) = std::env::var_os("FONTELLE_TEST_VOICEBANKS").map(PathBuf::from) else {
        eprintln!("FONTELLE_TEST_VOICEBANKS not set");
        return;
    };
    let installed = fontelle_vocal::voicebanks::list(&dir);
    for builtin in fontelle_vocal::builtin_backends() {
        for voice in (builtin.make)(&dir).voices() {
            let image = voice.image.as_deref().map(std::path::Path::new);
            let bank = fontelle_vocal::voicebanks::bank_for_voice(&installed, &voice.id, image)
                .unwrap_or_else(|| panic!("{} is no installed bank", voice.id));
            eprintln!(
                "{} -> {} ({}) {:?}",
                voice.id, bank.id, bank.name, bank.info.portrait
            );
            // Both show the bank's own picture — its icon (`image`) before
            // OpenUtau's large `portrait`, which a runtime may prefer, and a
            // file that is there (LIEE's yaml writes a Windows path) — so a
            // bank whose runtime has a picture has one here too.
            if image.is_some_and(|i| i.is_file()) {
                assert!(bank.info.portrait.is_some(), "{}", voice.id);
            }
        }
    }
}
