//! Singing a phrase end to end with a synthetic bank: in tune, in time,
//! deterministic, cancellable, cached, and the lanes doing what they say.

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use common::{FS, cents, measure_f0, midi_hz, rms};
use fontelle_sing::{
    Backend, CURVE_STEP, Curve, Lane, Phrase, PhraseNote, Quality, SingError, Sung,
};
use fontelle_utau::bank::Voicebank;
use fontelle_utau::cache::AnalysisCache;
use fontelle_utau::{
    NOTE_OPTION_COLOUR, SETTING_EXPRESSION, SETTING_PHONEMIZER, UtauBackend, sing_with,
};

const ALIASES: [&str; 8] = ["あ", "か", "さ", "ら", "い", "お", "- か", "a R"];

fn bank(name: &str) -> (PathBuf, Voicebank) {
    let root = common::temp_dir(name);
    let dir = root.join("Synth");
    std::fs::create_dir_all(&dir).unwrap();
    common::write_cv_bank(&dir, &ALIASES, 220.0);
    std::fs::write(dir.join("character.txt"), "name=Synth\n").unwrap();
    std::fs::write(dir.join("readme.txt"), "Free for tests only.\n").unwrap();
    let b = Voicebank::load(&dir).unwrap();
    (root, b)
}

fn phrase(notes: &[(f64, f64, f32, &str)]) -> Phrase {
    Phrase {
        voice: "utau/Synth".into(),
        language: "ja".into(),
        notes: notes
            .iter()
            .map(|&(s, d, p, l)| PhraseNote::new(s, d, p, l))
            .collect(),
        curves: BTreeMap::new(),
        extra_curves: BTreeMap::new(),
        // The written pitch exactly: the singer's own expression on it is
        // `natural.rs`'s.
        settings: BTreeMap::from([(SETTING_EXPRESSION.to_string(), 0.0)]),
        state: None,
        start: 0.0,
        note_ids: Vec::new(),
    }
}

fn sing(b: &Voicebank, cache: &AnalysisCache, p: &Phrase, q: Quality) -> Sung {
    sing_with(b, cache, p, q, 1, &|| false).unwrap()
}

/// Output samples between phrase times `a` and `b`.
fn span(s: &Sung, a: f64, b: f64) -> &[f32] {
    let i = (((a - s.start) * s.sample_rate as f64) as usize).min(s.samples.len());
    let j = (((b - s.start) * s.sample_rate as f64) as usize).min(s.samples.len());
    &s.samples[i..j]
}

#[test]
fn a_phrase_is_sung_on_its_notes_pitch() {
    let (_root, b) = bank("sing_pitch");
    let cache = AnalysisCache::new(None, 64 << 20);
    let p = phrase(&[
        (0.0, 0.6, 60.0, "か"),
        (0.6, 0.6, 64.0, "さ"),
        (1.2, 0.8, 55.0, "ら"),
    ]);
    let s = sing(&b, &cache, &p, Quality::Final);
    assert_eq!(s.sample_rate, FS);
    // The first consonant comes before the note: its preutterance.
    assert!(s.start < -0.05 && s.start > -0.2, "{}", s.start);
    for n in &p.notes {
        let f = measure_f0(
            &s.samples,
            FS,
            n.start + 0.2 - s.start,
            n.end() - 0.1 - s.start,
        );
        assert!(
            cents(f, midi_hz(n.pitch)).abs() < 10.0,
            "note {}: {f}",
            n.pitch
        );
    }
    assert!(rms(span(&s, 0.1, 1.9)) > 0.02);
    assert!(s.samples.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
    // The pitch reported is the one sung.
    assert!((s.pitch_step - CURVE_STEP).abs() < 1e-12);
    let at = |t: f64| s.pitch[((t - s.start) / s.pitch_step) as usize];
    assert!(cents(at(0.3), midi_hz(60.0)).abs() < 1.0, "{}", at(0.3));
    assert!(cents(at(1.6), midi_hz(55.0)).abs() < 1.0);
    // What each note was sung as.
    let aliases: Vec<Vec<String>> = s
        .phonemes
        .iter()
        .map(|n| n.phonemes.iter().map(|p| p.symbol.clone()).collect())
        .collect();
    assert_eq!(aliases, [vec!["- か"], vec!["さ"], vec!["ら"]]);
    let first = s.phonemes[0].phonemes[0].offset.unwrap();
    assert!((first - s.start).abs() < 0.02, "{first}");
}

#[test]
fn the_bend_lane_and_vibrato_move_the_pitch() {
    let (_root, b) = bank("sing_bend");
    let cache = AnalysisCache::new(None, 64 << 20);
    let mut p = phrase(&[(0.0, 1.0, 60.0, "あ")]);
    p.curves.insert(
        Lane::PitchBend,
        Curve {
            start: 0.0,
            step: CURVE_STEP,
            values: vec![100.0; 300],
        },
    );
    let s = sing(&b, &cache, &p, Quality::Final);
    let f = measure_f0(&s.samples, FS, 0.2 - s.start, 0.8 - s.start);
    assert!(cents(f, midi_hz(61.0)).abs() < 10.0, "{f}");
    let mut p = phrase(&[(0.0, 1.5, 60.0, "あ")]);
    p.notes[0].vibrato = Some(fontelle_sing::Vibrato {
        depth: 50.0,
        rate: 5.0,
        delay: 0.2,
        fade_in: 0.0,
        fade_out: 0.0,
    });
    let s = sing(&b, &cache, &p, Quality::Final);
    let max = s.pitch.iter().cloned().fold(0.0f32, f32::max);
    assert!(cents(max, midi_hz(60.5)).abs() < 3.0, "{max}");
}

#[test]
fn singing_is_deterministic_and_preview_is_close() {
    let (_root, b) = bank("sing_det");
    let cache = AnalysisCache::new(None, 64 << 20);
    let p = phrase(&[(0.0, 0.5, 62.0, "か"), (0.5, 0.5, 62.0, "あ")]);
    let a = sing(&b, &cache, &p, Quality::Final);
    let again = sing(&b, &cache, &p, Quality::Final);
    assert_eq!(a, again);
    let prev = sing(&b, &cache, &p, Quality::Preview);
    assert!(!prev.samples.is_empty());
    let f = measure_f0(
        &prev.samples,
        prev.sample_rate,
        0.6 - prev.start,
        0.9 - prev.start,
    );
    let db = 20.0 * (rms(&prev.samples) / rms(&a.samples)).log10();
    assert!(db.abs() < 1.0, "a preview as loud as the final: {db} dB");
    assert!(cents(f, midi_hz(62.0)).abs() < 15.0, "{f}");
}

#[test]
fn a_raised_cancel_flag_stops_the_render() {
    let (_root, b) = bank("sing_cancel");
    let cache = AnalysisCache::new(None, 64 << 20);
    let p = phrase(&[(0.0, 0.5, 60.0, "か")]);
    let r = sing_with(&b, &cache, &p, Quality::Final, 1, &|| true);
    assert_eq!(r.unwrap_err(), SingError::Cancelled);
}

#[test]
fn dynamics_gender_and_breathiness_change_what_they_say() {
    let (_root, b) = bank("sing_lanes");
    let cache = AnalysisCache::new(None, 64 << 20);
    let base = phrase(&[(0.0, 1.0, 60.0, "あ")]);
    let flat = sing(&b, &cache, &base, Quality::Final);
    let lane = |lane: Lane, v: f32| {
        let mut p = base.clone();
        p.curves.insert(
            lane,
            Curve {
                start: -0.5,
                step: CURVE_STEP,
                values: vec![v; 400],
            },
        );
        sing(&b, &cache, &p, Quality::Final)
    };
    let loud = lane(Lane::Dynamics, 6.0);
    let db = 20.0 * (rms(span(&loud, 0.2, 0.8)) / rms(span(&flat, 0.2, 0.8))).log10();
    assert!((db - 6.0).abs() < 1.0, "{db}");
    // Gender up: the spectrum's centroid rises; the pitch does not move.
    let bright = lane(Lane::Gender, 300.0);
    let c0 = centroid(span(&flat, 0.2, 0.8));
    let c1 = centroid(span(&bright, 0.2, 0.8));
    assert!(c1 > c0 * 1.08, "{c0} -> {c1}");
    let f = measure_f0(&bright.samples, FS, 0.2 - bright.start, 0.8 - bright.start);
    assert!(cents(f, midi_hz(60.0)).abs() < 10.0);
    // Breathier: more energy between the harmonics.
    let breathy = lane(Lane::Breathiness, 1.0);
    assert!(
        hnr(span(&breathy, 0.2, 0.8), midi_hz(60.0))
            < hnr(span(&flat, 0.2, 0.8), midi_hz(60.0)) - 3.0
    );
    // Tension: brighter when pressed.
    let pressed = lane(Lane::Tension, 1.0);
    assert!(centroid(span(&pressed, 0.2, 0.8)) > c0 * 1.05);
}

/// Spectral centroid, Hz.
fn centroid(x: &[f32]) -> f32 {
    let (mut num, mut den) = (0.0, 0.0);
    for (f, p) in power_spectrum(x) {
        num += f * p;
        den += p;
    }
    num / den
}

/// Harmonic-to-noise ratio, dB: power near harmonics of f0 over power
/// between them, below 5 kHz.
fn hnr(x: &[f32], f0: f32) -> f32 {
    let (mut h, mut n) = (0.0, 0.0);
    for (f, p) in power_spectrum(x) {
        if f > 5000.0 || f < f0 * 0.5 {
            continue;
        }
        let k = f / f0;
        if (k - k.round()).abs() < 0.15 {
            h += p;
        } else if (k - k.round()).abs() > 0.3 {
            n += p;
        }
    }
    10.0 * (h / n).log10()
}

/// A plain DFT power spectrum of a Hann-windowed 8192-sample stretch.
fn power_spectrum(x: &[f32]) -> Vec<(f32, f32)> {
    let n = 8192.min(x.len());
    let x = &x[..n];
    let mut out = Vec::new();
    for k in (1..n / 2).step_by(2) {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &v) in x.iter().enumerate() {
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
            let a = std::f64::consts::TAU * (k * i) as f64 / n as f64;
            re += v as f64 * w * a.cos();
            im -= v as f64 * w * a.sin();
        }
        out.push((k as f32 * FS as f32 / n as f32, (re * re + im * im) as f32));
    }
    out
}

#[test]
fn analyses_are_kept_on_disk_and_reused() {
    let (root, b) = bank("sing_cache");
    let dir = root.join("cache");
    let p = phrase(&[(0.0, 0.5, 60.0, "か"), (0.5, 0.5, 60.0, "さ")]);
    let cold = AnalysisCache::new(Some(dir.clone()), 64 << 20);
    let t = Instant::now();
    let a = sing(&b, &cold, &p, Quality::Final);
    let cold_time = t.elapsed();
    assert!(
        std::fs::read_dir(&dir).unwrap().count() > 0,
        "written to disk"
    );
    // The second note sings さ (the first, "- か").
    let file = &b.find("さ", 60, "").unwrap().file;
    assert!(cold.is_cached(file));
    // A new session's cache reads them back.
    let warm = AnalysisCache::new(Some(dir), 64 << 20);
    assert!(warm.is_cached(file));
    let t = Instant::now();
    let c = sing(&b, &warm, &p, Quality::Final);
    let warm_time = t.elapsed();
    assert_eq!(a.samples.len(), c.samples.len());
    let diff = a
        .samples
        .iter()
        .zip(&c.samples)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max);
    assert!(diff < 0.01, "an analysis from disk sings the same: {diff}");
    eprintln!("cold {cold_time:?}, warm {warm_time:?}");
}

#[test]
fn the_backend_lists_banks_with_their_terms_and_sings_them() {
    let (root, _b) = bank("sing_backend");
    let backend = UtauBackend::new(vec![root.clone()], Some(root.join("cache")));
    let voices = backend.voices();
    assert_eq!(voices.len(), 1, "{voices:?}");
    let v = &voices[0];
    assert_eq!(v.id, "utau/Synth");
    assert_eq!(v.name, "Synth");
    assert_eq!(v.languages.first().map(String::as_str), Some("ja"));
    assert_eq!(
        v.terms.as_deref().map(str::trim),
        Some("Free for tests only.")
    );
    assert!(!v.research_only);
    let caps = backend.capabilities(&v.id);
    for lane in [
        Lane::PitchBend,
        Lane::Dynamics,
        Lane::Breathiness,
        Lane::Gender,
        Lane::Tension,
    ] {
        assert!(caps.lanes.contains(&lane), "{lane:?}");
    }
    assert!(caps.vibrato && caps.portamento && caps.velocity && caps.phoneme_timing);
    assert!(caps.phonemes.contains(&"か".to_string()));
    assert!(caps.settings.iter().any(|s| s.id == SETTING_PHONEMIZER));
    assert!(caps.note_options.iter().all(|o| o.id == NOTE_OPTION_COLOUR));
    assert!(!backend.version().is_empty());
    let mut session = backend.session().unwrap();
    let s = session
        .sing(
            &phrase(&[(0.0, 0.5, 60.0, "か")]),
            Quality::Preview,
            3,
            &|| false,
        )
        .unwrap();
    assert!(rms(&s.samples) > 0.01);
    let mut unknown = phrase(&[(0.0, 0.5, 60.0, "か")]);
    unknown.voice = "utau/Nope".into();
    assert!(matches!(
        session.sing(&unknown, Quality::Preview, 3, &|| false),
        Err(SingError::Failed(_))
    ));
}

#[test]
fn the_phonemizer_setting_overrides_detection() {
    let (_root, b) = bank("sing_setting");
    let cache = AnalysisCache::new(None, 64 << 20);
    let mut p = phrase(&[(0.0, 0.5, 60.0, "か")]);
    let auto = sing(&b, &cache, &p, Quality::Preview);
    assert_eq!(auto.phonemes[0].phonemes.len(), 1, "CV: no ending");
    p.settings.insert(
        SETTING_PHONEMIZER.into(),
        fontelle_utau::phonemize::Phonemizer::JaVcv.setting_value(),
    );
    let vcv = sing(&b, &cache, &p, Quality::Preview);
    let sung: Vec<&str> = vcv.phonemes[0]
        .phonemes
        .iter()
        .map(|p| p.symbol.as_str())
        .collect();
    assert_eq!(sung, ["- か", "a R"]);
}

/// A bank names its folder as what it is sung from (its loudness is kept
/// by the folder's content); a voice it does not have names nothing.
#[test]
fn a_bank_is_sung_from_its_folder() {
    let (root, _b) = bank("sing_backend_files");
    let backend = UtauBackend::new(vec![root.clone()], Some(root.join("cache")));
    assert_eq!(backend.voice_files("utau/Synth"), vec![root.join("Synth")]);
    assert!(backend.voice_files("utau/Nobody").is_empty());
}
