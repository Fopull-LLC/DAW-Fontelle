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

/// What keeping a voice's level on disk saves its first phrase in a
/// session, with its models already open (or its samples analysed): the
/// reference sung before it. Prints, per voice, the first phrase with the
/// level measured, then in a new session with the level kept.
#[test]
#[ignore = "needs FONTELLE_TEST_VOICEBANKS"]
fn a_kept_level_spares_the_first_phrase_its_reference() {
    use std::sync::Arc;
    use std::time::Instant;
    let Some(dir) = std::env::var_os("FONTELLE_TEST_VOICEBANKS").map(PathBuf::from) else {
        eprintln!("FONTELLE_TEST_VOICEBANKS not set");
        return;
    };
    let only = std::env::var("FONTELLE_TEST_VOICE").ok();
    let levels = std::env::temp_dir().join(format!("fontelle-kept-levels-{}", std::process::id()));
    std::fs::remove_dir_all(&levels).ok();
    // Analyses are kept across runs, as the studio keeps them: run twice
    // to time a UTAU bank's levelling with its samples analysed.
    let utau_cache = std::env::temp_dir().join("fontelle-kept-levels-analyses");
    type Make<'a> = Box<dyn Fn() -> Arc<dyn fontelle_sing::Backend> + 'a>;
    let make: [(&str, Make); 2] = [
        (
            "diffsinger",
            Box::new(|| {
                Arc::new(fontelle_diffsinger::DiffSinger::new(
                    fontelle_diffsinger::Options::new(vec![dir.clone()]),
                ))
            }),
        ),
        (
            "utau",
            Box::new(|| {
                Arc::new(fontelle_utau::UtauBackend::new(
                    vec![dir.clone()],
                    Some(utau_cache.clone()),
                ))
            }),
        ),
    ];
    for (kind, make) in &make {
        for voice in make().voices() {
            if only
                .as_ref()
                .is_some_and(|o| !voice.id.contains(o.as_str()))
            {
                continue;
            }
            let language = voice.languages.first().cloned().unwrap_or_default();
            let lyric = match language.as_str() {
                "en" => "la",
                "zh" => "la",
                _ => "ら",
            };
            let mid = f32::from(voice.lowest.max(48).midpoint(voice.highest.min(76)));
            let notes = (0..4)
                .map(|i| PhraseNote::new(f64::from(i) * 0.5, 0.5, mid, lyric))
                .collect();
            let phrase = Phrase::new(voice.id.clone(), language, notes);
            let time = |kept: bool| {
                let inner = make();
                // Open the models / analyse the samples first: what is
                // timed is the levelling alone.
                let _ = inner
                    .session()
                    .unwrap()
                    .sing(&phrase, Quality::Preview, 7, &|| false);
                let backend =
                    fontelle_vocal::leveled_kept(inner, kept.then(|| levels.join("kept")));
                let mut session = backend.session().unwrap();
                let at = Instant::now();
                let ok = session
                    .sing(&phrase, Quality::Preview, 1, &|| false)
                    .is_ok();
                (at.elapsed().as_secs_f64() * 1000.0, ok)
            };
            // Measured (nothing kept yet, so this one writes it) …
            let (measured, ok) = time(true);
            // … then a new session finds it.
            let (kept, _) = time(true);
            println!(
                "{kind:10} {:40} first phrase: level measured {measured:6.0} ms, kept {kept:6.0} ms{}",
                voice.id,
                if ok { "" } else { " (failed)" }
            );
        }
    }
}
