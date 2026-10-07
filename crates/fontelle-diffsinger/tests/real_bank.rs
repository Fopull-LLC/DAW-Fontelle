//! Singing with a real community voicebank. Ignored by default: the banks
//! are not in the tree. Run with
//!
//! ```text
//! FONTELLE_TEST_VOICEBANKS=/path/to/voicebanks \
//!     cargo test --release -p fontelle-diffsinger --test real_bank -- --ignored --nocapture
//! ```
//!
//! The folder holds voicebank folders and a `Dependencies` folder with their
//! vocoders. `FONTELLE_TEST_VOICE` picks a bank (else the first that sings
//! English); `FONTELLE_TEST_RENDERS` names a folder to write the takes to.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use fontelle_diffsinger::{DiffSinger, Options};
use fontelle_sing::{Backend, Curve, Lane, Phrase, PhraseNote, Quality, SingError, Sung};

fn singer() -> Option<(DiffSinger, String)> {
    let Ok(dir) = std::env::var("FONTELLE_TEST_VOICEBANKS") else {
        eprintln!("FONTELLE_TEST_VOICEBANKS is not set; nothing to sing with");
        return None;
    };
    let singer = DiffSinger::new(Options::new(vec![dir.into()]));
    let voices = singer.voices();
    let voice = std::env::var("FONTELLE_TEST_VOICE")
        .ok()
        .unwrap_or_else(|| {
            voices
                .iter()
                .find(|v| v.languages.iter().any(|l| l == "en"))
                .or(voices.first())
                .expect("no voicebank in FONTELLE_TEST_VOICEBANKS")
                .id
                .clone()
        });
    assert!(
        singer.problems(&voice).is_empty(),
        "{voice}: {:?}",
        singer.problems(&voice)
    );
    Some((singer, voice))
}

/// "Amazing grace, how sweet the sound": eight notes, five seconds.
fn phrase(voice: &str) -> Phrase {
    let notes = [
        ("a-", 60.0, 0.4),
        ("ma-", 65.0, 0.6),
        ("zing", 69.0, 0.5),
        ("grace", 67.0, 1.0),
        ("how", 65.0, 0.5),
        ("sweet", 64.0, 0.5),
        ("the", 62.0, 0.5),
        ("sound", 60.0, 1.0),
    ];
    let mut t = 0.0;
    let notes = notes
        .iter()
        .map(|&(l, p, d)| {
            let n = PhraseNote::new(t, d, p, l);
            t += d;
            n
        })
        .collect();
    Phrase {
        voice: voice.into(),
        language: "en".into(),
        notes,
        curves: BTreeMap::new(),
        extra_curves: BTreeMap::new(),
        settings: BTreeMap::new(),
        state: None,
        start: 0.0,
        note_ids: Vec::new(),
    }
}

fn sing(singer: &DiffSinger, p: &Phrase, quality: Quality, seed: u64) -> (Sung, f64) {
    let mut session = singer.session().unwrap();
    let t = Instant::now();
    let sung = session.sing(p, quality, seed, &|| false).unwrap();
    (sung, t.elapsed().as_secs_f64())
}

fn save(name: &str, sung: &Sung) {
    let Ok(dir) = std::env::var("FONTELLE_TEST_RENDERS") else {
        return;
    };
    let mut out = Vec::new();
    let data = (sung.samples.len() * 2) as u32;
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sung.sample_rate.to_le_bytes());
    out.extend_from_slice(&(sung.sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in &sung.samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(std::path::Path::new(&dir).join(format!("{name}.wav")), out).unwrap();
}

/// No NaN, no clipping, not silent.
fn assert_healthy(sung: &Sung, what: &str) {
    assert!(!sung.samples.is_empty(), "{what}: no samples");
    assert!(
        sung.samples.iter().all(|s| s.is_finite()),
        "{what}: NaN or inf"
    );
    let clipped = sung.samples.iter().filter(|s| s.abs() >= 0.999).count();
    assert!(
        clipped * 1000 < sung.samples.len(),
        "{what}: {clipped} clipped samples"
    );
    let rms = (sung.samples.iter().map(|s| s * s).sum::<f32>() / sung.samples.len() as f32).sqrt();
    assert!(rms > 0.01, "{what}: nearly silent (rms {rms})");
    let peak = sung.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    eprintln!("{what}: rms {rms:.3}, peak {peak:.3}");
}

/// YIN's f0 estimate of one window, if it is voiced.
fn yin(window: &[f32], rate: f32) -> Option<f32> {
    let (min_lag, max_lag) = ((rate / 1000.0) as usize, (rate / 70.0) as usize);
    let n = window.len() - max_lag;
    let mut d = vec![0f32; max_lag + 1];
    for (lag, slot) in d.iter_mut().enumerate().skip(1) {
        *slot = (0..n).map(|i| (window[i] - window[i + lag]).powi(2)).sum();
    }
    let mut running = 0f32;
    let mut cmnd = vec![1f32; max_lag + 1];
    for lag in 1..=max_lag {
        running += d[lag];
        cmnd[lag] = d[lag] * lag as f32 / running.max(1e-12);
    }
    let lag = (min_lag..max_lag).find(|&l| cmnd[l] < 0.15 && cmnd[l] <= cmnd[l + 1])?;
    // Parabolic refinement.
    let (a, b, c) = (cmnd[lag - 1], cmnd[lag], cmnd[lag + 1]);
    let shift = 0.5 * (a - c) / (a - 2.0 * b + c).max(1e-12);
    Some(rate / (lag as f32 + shift.clamp(-1.0, 1.0)))
}

fn cents(hz: f32, midi: f32) -> f32 {
    1200.0 * (hz / fontelle_diffsinger::curves::midi_to_hz(midi)).log2()
}

/// The median sung pitch over the middle of each note, as cents from it.
fn note_errors(p: &Phrase, sung: &Sung) -> Vec<Option<f32>> {
    let rate = sung.sample_rate as f32;
    let window = 2048;
    p.notes
        .iter()
        .map(|n| {
            let from = n.start + n.duration * 0.25;
            let to = n.end() - n.duration * 0.25;
            let mut found = Vec::new();
            let mut t = from;
            while t < to {
                let i = ((t - sung.start) * f64::from(rate)) as usize;
                if i + window <= sung.samples.len()
                    && let Some(f) = yin(&sung.samples[i..i + window], rate)
                {
                    found.push(cents(f, n.pitch));
                }
                t += 0.01;
            }
            if found.len() < 3 {
                return None;
            }
            found.sort_by(f32::total_cmp);
            Some(found[found.len() / 2])
        })
        .collect()
}

#[test]
#[ignore = "needs a real voicebank: FONTELLE_TEST_VOICEBANKS"]
fn a_real_bank_sings_the_notes_it_is_given() {
    let Some((singer, voice)) = singer() else {
        return;
    };
    let t = Instant::now();
    singer.preload(&voice).unwrap();
    eprintln!(
        "{voice}: models opened in {:.2} s",
        t.elapsed().as_secs_f64()
    );
    let base = phrase(&voice);
    let seconds = base.notes.last().unwrap().end();

    // The notes' own line: the sung pitch must be the notes'.
    let mut notes_pitch = base.clone();
    notes_pitch.settings.insert("natural_pitch".into(), 0.0);
    let (sung, took) = sing(&singer, &notes_pitch, Quality::Final, 7);
    eprintln!("final, notes' pitch: {seconds:.1} s phrase in {took:.2} s");
    save("notes_pitch_final", &sung);
    assert_healthy(&sung, "final, notes' pitch");
    let errors = note_errors(&notes_pitch, &sung);
    eprintln!("cents off each note: {errors:?}");
    let close = errors.iter().flatten().filter(|e| e.abs() < 35.0).count();
    assert!(close * 10 >= notes_pitch.notes.len() * 9, "{errors:?}");

    // The bank's own pitch model: near the notes, if not on them.
    let (sung, took) = sing(&singer, &base, Quality::Final, 7);
    eprintln!("final, model pitch: {seconds:.1} s phrase in {took:.2} s");
    save("model_pitch_final", &sung);
    assert_healthy(&sung, "final, model pitch");
    let errors = note_errors(&base, &sung);
    eprintln!("cents off each note: {errors:?}");
    let close = errors.iter().flatten().filter(|e| e.abs() < 100.0).count();
    assert!(close * 4 >= base.notes.len() * 3, "{errors:?}");
    // The reported pitch is what was sung.
    let rate = sung.sample_rate as f32;
    let mut diffs = Vec::new();
    for (k, &hz) in sung.pitch.iter().enumerate() {
        let t = k as f64 * sung.pitch_step;
        let i = (t * f64::from(rate)) as usize;
        if hz > 0.0
            && i + 2048 <= sung.samples.len()
            && let Some(f) = yin(&sung.samples[i..i + 2048], rate)
        {
            // The analysis window is centred 23 ms later; compare to the
            // reported pitch there.
            let at =
                (k + (1024.0 / rate / sung.pitch_step as f32) as usize).min(sung.pitch.len() - 1);
            if sung.pitch[at] > 0.0 {
                diffs.push((1200.0 * (f / sung.pitch[at]).log2()).abs());
            }
        }
    }
    diffs.sort_by(f32::total_cmp);
    let median = diffs[diffs.len() / 2];
    eprintln!(
        "reported vs sung pitch: median {median:.1} cents over {} frames",
        diffs.len()
    );
    assert!(median < 50.0);

    // What the roll is told.
    assert_eq!(sung.phonemes.len(), base.notes.len());
    assert!(!sung.phonemes[3].phonemes.is_empty(), "grace");
    assert!(!sung.phonemes[3].guessed, "grace is in the dictionary");
    assert!(sung.phonemes[3].phonemes[0].offset.unwrap() < 0.0);
    assert!(sung.start < 0.0, "the onset starts before the first note");

    // Preview: quicker, same health.
    let (preview, took) = sing(&singer, &base, Quality::Preview, 7);
    eprintln!("preview, model pitch: {seconds:.1} s phrase in {took:.2} s");
    save("model_pitch_preview", &preview);
    assert_healthy(&preview, "preview");
}

#[test]
#[ignore = "needs a real voicebank: FONTELLE_TEST_VOICEBANKS"]
fn a_real_bank_is_deterministic_and_cancellable() {
    let Some((singer, voice)) = singer() else {
        return;
    };
    let base = phrase(&voice);
    let (a, _) = sing(&singer, &base, Quality::Preview, 11);
    let (b, _) = sing(&singer, &base, Quality::Preview, 11);
    assert_eq!(a.samples, b.samples, "the same seed sings the same take");
    let (c, _) = sing(&singer, &base, Quality::Preview, 12);
    assert_ne!(a.samples, c.samples, "another seed, another take");

    let flag = Arc::new(AtomicBool::new(false));
    let raise = flag.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        raise.store(true, Ordering::Relaxed);
    });
    let mut session = singer.session().unwrap();
    let t = Instant::now();
    let r = session.sing(&base, Quality::Final, 1, &|| flag.load(Ordering::Relaxed));
    let took = t.elapsed();
    eprintln!("cancelled after {took:?}");
    assert_eq!(r, Err(SingError::Cancelled));
    assert!(took < Duration::from_secs(2), "{took:?}");
}

#[test]
#[ignore = "needs a real voicebank: FONTELLE_TEST_VOICEBANKS"]
fn a_real_bank_answers_to_velocity_lanes_and_dynamics() {
    let Some((singer, voice)) = singer() else {
        return;
    };
    let base = phrase(&voice);
    let onset = |p: &Phrase| {
        let (s, _) = sing(&singer, p, Quality::Preview, 3);
        s.phonemes[5].phonemes[0].offset.unwrap()
    };
    let mut fast = base.clone();
    fast.notes[5].velocity = 2.0;
    let mut slow = base.clone();
    slow.notes[5].velocity = 0.5;
    let (fast_onset, slow_onset) = (onset(&fast), onset(&slow));
    eprintln!("\"sweet\" onset: {fast_onset:.3} s at velocity 2, {slow_onset:.3} s at 0.5");
    assert!(fast_onset > slow_onset, "a quicker consonant starts later");

    // Dynamics is gain on the same take.
    let (plain, _) = sing(&singer, &base, Quality::Preview, 5);
    let mut quiet = base.clone();
    quiet.curves.insert(
        Lane::Dynamics,
        Curve {
            start: -1.0,
            step: 0.005,
            values: vec![-6.0206],
        },
    );
    let (q, _) = sing(&singer, &quiet, Quality::Preview, 5);
    let rms =
        |s: &Sung| (s.samples.iter().map(|x| x * x).sum::<f32>() / s.samples.len() as f32).sqrt();
    let ratio = rms(&q) / rms(&plain);
    assert!((ratio - 0.5).abs() < 0.01, "{ratio}");

    // The lanes the bank declares change the take and keep it healthy.
    let caps = singer.capabilities(&voice);
    for lane in [
        Lane::Breathiness,
        Lane::Voicing,
        Lane::Tension,
        Lane::Gender,
    ] {
        if !caps.lanes.contains(&lane) {
            continue;
        }
        let mut p = base.clone();
        let value = if lane == Lane::Gender { 300.0 } else { 0.6 };
        p.curves.insert(
            lane,
            Curve {
                start: -1.0,
                step: 0.005,
                values: vec![value],
            },
        );
        let (s, _) = sing(&singer, &p, Quality::Preview, 5);
        save(&format!("lane_{lane:?}"), &s);
        assert_healthy(&s, &format!("{lane:?}"));
        assert_ne!(s.samples, plain.samples, "{lane:?} changed nothing");
    }
}

#[test]
#[ignore = "needs a real voicebank: FONTELLE_TEST_VOICEBANKS"]
fn a_real_multi_speaker_bank_sings_kana_with_each_speaker() {
    let Ok(dir) = std::env::var("FONTELLE_TEST_VOICEBANKS") else {
        eprintln!("FONTELLE_TEST_VOICEBANKS is not set; nothing to sing with");
        return;
    };
    let singer = DiffSinger::new(Options::new(vec![dir.into()]));
    let Some((voice, speakers)) = singer.voices().into_iter().find_map(|v| {
        let caps = singer.capabilities(&v.id);
        let option = caps.note_options.into_iter().find(|o| o.id == "speaker")?;
        v.languages
            .iter()
            .any(|l| l == "ja")
            .then_some((v.id, option.choices))
    }) else {
        eprintln!("no Japanese voicebank with several speakers; nothing to test");
        return;
    };
    eprintln!("{voice}: speakers {speakers:?}");
    let lyrics = ["さ", "く", "ら", "さ", "く", "ら", "の", "はな"];
    let pitches = [69.0, 69.0, 71.0, 69.0, 69.0, 71.0, 69.0, 71.0];
    let mut base = Phrase {
        voice: voice.clone(),
        language: "ja".into(),
        notes: lyrics
            .iter()
            .zip(pitches)
            .enumerate()
            .map(|(i, (l, p))| PhraseNote::new(i as f64 * 0.55, 0.55, p - 12.0, *l))
            .collect(),
        curves: BTreeMap::new(),
        extra_curves: BTreeMap::new(),
        settings: BTreeMap::new(),
        state: None,
        start: 0.0,
        note_ids: Vec::new(),
    };
    base.settings.insert("natural_pitch".into(), 0.0);
    let mut takes = Vec::new();
    for (id, label) in &speakers {
        let mut p = base.clone();
        for n in &mut p.notes {
            n.options.insert("speaker".into(), id.clone());
        }
        let (sung, took) = sing(&singer, &p, Quality::Final, 9);
        eprintln!(
            "{label}: {:.1} s phrase in {took:.2} s",
            p.notes.last().unwrap().end()
        );
        save(&format!("speaker_{label}"), &sung);
        assert_healthy(&sung, label);
        assert!(
            sung.phonemes.iter().all(|n| !n.guessed),
            "kana are in the dictionary"
        );
        let errors = note_errors(&p, &sung);
        eprintln!("cents off each note: {errors:?}");
        let close = errors.iter().flatten().filter(|e| e.abs() < 35.0).count();
        assert!(close * 10 >= p.notes.len() * 8, "{errors:?}");
        takes.push(sung.samples);
    }
    assert_ne!(takes[0], takes[1], "the speakers sound different");
    // Half of the second speaker drawn over the first.
    let mut mixed = base.clone();
    mixed.extra_curves.insert(
        format!("speaker:{}", speakers[1].0),
        Curve {
            start: -1.0,
            step: 0.005,
            values: vec![50.0],
        },
    );
    let (sung, _) = sing(&singer, &mixed, Quality::Final, 9);
    save("speaker_mix", &sung);
    assert_healthy(&sung, "mix");
    assert!(sung.samples != takes[0] && sung.samples != takes[1]);
}

#[test]
#[ignore = "needs a real voicebank: FONTELLE_TEST_VOICEBANKS"]
fn a_real_chinese_bank_sings_chinese_characters() {
    let Ok(dir) = std::env::var("FONTELLE_TEST_VOICEBANKS") else {
        eprintln!("FONTELLE_TEST_VOICEBANKS is not set; nothing to sing with");
        return;
    };
    let singer = DiffSinger::new(Options::new(vec![dir.into()]));
    let voices: Vec<String> = singer
        .voices()
        .into_iter()
        .filter(|v| v.languages.iter().any(|l| l == "zh"))
        .map(|v| v.id)
        .collect();
    if voices.is_empty() {
        eprintln!("no Chinese voicebank; nothing to test");
        return;
    }
    // 我们一起去银行: 行 is hang only beside 银, on the note before.
    let lyrics = ["我", "们", "一", "起", "去", "银", "行"];
    let pitches = [62.0, 64.0, 66.0, 67.0, 69.0, 67.0, 66.0];
    for voice in voices {
        let p = Phrase {
            voice: voice.clone(),
            language: "zh".into(),
            notes: lyrics
                .iter()
                .zip(pitches)
                .enumerate()
                .map(|(i, (l, p))| PhraseNote::new(i as f64 * 0.45, 0.45, p - 5.0, *l))
                .collect(),
            curves: BTreeMap::new(),
            extra_curves: BTreeMap::new(),
            settings: BTreeMap::new(),
            state: None,
            start: 0.0,
            note_ids: Vec::new(),
        };
        let (sung, took) = sing(&singer, &p, Quality::Final, 3);
        let said: Vec<String> = sung
            .phonemes
            .iter()
            .map(|n| {
                n.phonemes
                    .iter()
                    .map(|p| p.symbol.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        eprintln!("{voice}: {said:?} in {took:.2} s");
        save(&format!("zh_{}", voice.replace(['/', ' '], "_")), &sung);
        assert_healthy(&sung, &voice);
        assert!(
            sung.phonemes.iter().all(|n| !n.guessed),
            "{voice}: every character is read: {said:?}"
        );
        assert!(
            said[6].contains("ang"),
            "{voice}: 银行 is yin hang: {said:?}"
        );
    }
}
