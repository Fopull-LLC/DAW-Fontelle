//! Singing that sounds sung: the pitch a singer makes of the written
//! notes (a scoop into a phrase, overshoot after a leap, a late vibrato on
//! long notes, a small fall at the end), units joined without a jump in
//! level or colour, a breath before a phrase, and an aspirated release.
//!
//! Ty, of Kasane Teto's English bank: "i definitely think it sounds better
//! ... but i still think it could be more natural sounding."

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::{FS, cents, measure_f0, midi_hz, rms};
use fontelle_sing::{Phrase, PhraseNote, Portamento, Quality, Sung, Vibrato, note_pitch};
use fontelle_utau::bank::Voicebank;
use fontelle_utau::cache::{self, AnalysisCache};
use fontelle_utau::expression::pitch_line;
use fontelle_utau::render::{self, UnitSource, Voicing};
use fontelle_utau::{SETTING_BREATH, SETTING_EXPRESSION, phonemize, sing_with};

fn notes(list: &[(f64, f64, f32)]) -> Vec<PhraseNote> {
    list.iter()
        .map(|&(s, d, p)| PhraseNote::new(s, d, p, "a"))
        .collect()
}

/// Cents of `line` against `midi` at `t`.
fn dev(line: &fontelle_sing::Curve, t: f64, midi: f32) -> f32 {
    (line.at(t, 0.0) - midi) * 100.0
}

fn range(line: &fontelle_sing::Curve, a: f64, b: f64, midi: f32) -> (f32, f32) {
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    let mut t = a;
    while t < b {
        let d = dev(line, t, midi);
        lo = lo.min(d);
        hi = hi.max(d);
        t += 0.005;
    }
    (lo, hi)
}

#[test]
fn without_expression_the_line_is_the_written_pitch() {
    let n = notes(&[(0.0, 0.6, 60.0), (0.6, 1.4, 67.0), (2.4, 0.5, 62.0)]);
    let a = pitch_line(&n, -0.2, 3.0, 0.0);
    let b = note_pitch(&n, -0.2, 3.0);
    assert_eq!(a, b);
}

#[test]
fn a_long_note_gets_a_late_vibrato_and_stays_in_tune() {
    let n = notes(&[(0.0, 2.0, 60.0)]);
    let line = pitch_line(&n, 0.0, 2.0, 1.0);
    // Settled, and no vibrato yet, early in the note.
    let (lo, hi) = range(&line, 0.12, 0.3, 60.0);
    assert!(lo > -15.0 && hi < 15.0, "{lo} {hi}");
    // A vibrato later, around the note.
    let (lo, hi) = range(&line, 1.0, 1.8, 60.0);
    assert!(hi - lo > 40.0 && hi - lo < 90.0, "{lo} {hi}");
    let mut sum = 0.0;
    let mut crossings = 0;
    let mut prev = dev(&line, 1.0, 60.0);
    let mut t = 1.0;
    while t < 1.8 {
        let d = dev(&line, t, 60.0);
        sum += d;
        if (d > 0.0) != (prev > 0.0) {
            crossings += 1;
        }
        prev = d;
        t += 0.005;
    }
    assert!((sum / 160.0).abs() < 8.0, "mean {}", sum / 160.0);
    // 5 to 6.5 Hz over 0.8 s: 4 to 5.2 cycles, two crossings each.
    assert!((8..=11).contains(&crossings), "{crossings}");
}

#[test]
fn a_short_note_gets_no_vibrato() {
    let n = notes(&[(0.0, 0.3, 60.0), (0.3, 0.3, 60.0)]);
    let line = pitch_line(&n, 0.0, 0.6, 1.0);
    let (lo, hi) = range(&line, 0.12, 0.28, 60.0);
    assert!(hi - lo < 15.0, "{lo} {hi}");
}

#[test]
fn a_leap_overshoots_then_settles() {
    let n = notes(&[(0.0, 0.5, 60.0), (0.5, 1.0, 67.0)]);
    let line = pitch_line(&n, 0.0, 1.5, 1.0);
    let (_, hi) = range(&line, 0.5, 0.7, 67.0);
    assert!(hi > 5.0 && hi < 60.0, "overshoot {hi}");
    let (lo, hi) = range(&line, 0.72, 0.78, 67.0);
    assert!(lo > -15.0 && hi < 15.0, "{lo} {hi}");
    // Down, it overshoots below.
    let n = notes(&[(0.0, 0.5, 67.0), (0.5, 1.0, 60.0)]);
    let line = pitch_line(&n, 0.0, 1.5, 1.0);
    let (lo, _) = range(&line, 0.5, 0.7, 60.0);
    assert!(lo < -5.0 && lo > -60.0, "overshoot {lo}");
}

#[test]
fn a_phrase_starts_from_below_and_ends_with_a_small_fall() {
    let n = notes(&[(0.0, 0.8, 60.0), (1.5, 0.8, 64.0)]);
    let line = pitch_line(&n, 0.0, 2.4, 1.0);
    assert!(dev(&line, 0.01, 60.0) < -10.0, "{}", dev(&line, 0.01, 60.0));
    assert!(
        dev(&line, 0.15, 60.0).abs() < 8.0,
        "{}",
        dev(&line, 0.15, 60.0)
    );
    // After a rest, again.
    assert!(dev(&line, 1.51, 64.0) < -10.0);
    // The end of a run falls a little.
    assert!(dev(&line, 0.79, 60.0) < -5.0, "{}", dev(&line, 0.79, 60.0));
    assert!(dev(&line, 2.29, 64.0) < -5.0);
}

#[test]
fn the_line_is_deterministic_and_keeps_what_the_writer_set() {
    let mut n = notes(&[(0.0, 0.5, 60.0), (0.5, 2.0, 64.0)]);
    let a = pitch_line(&n, 0.0, 2.5, 1.0);
    assert_eq!(a, pitch_line(&n, 0.0, 2.5, 1.0));
    // A vibrato and a glide the writer set are sung as written: no second
    // vibrato on top, no overshoot added to their glide.
    n[1].vibrato = Some(Vibrato {
        depth: 50.0,
        rate: 6.0,
        delay: 0.2,
        fade_in: 0.1,
        fade_out: 0.1,
    });
    n[1].portamento = Some(Portamento {
        length: 0.1,
        position: 0.5,
        shape: fontelle_sing::GlideShape::Linear,
    });
    let line = pitch_line(&n, 0.0, 2.5, 1.0);
    let written = note_pitch(&n, 0.0, 2.5);
    let mut t = 0.5;
    while t < 2.4 {
        let d = (line.at(t, 0.0) - written.at(t, 0.0)) * 100.0;
        assert!(d.abs() < 10.0, "{t}: {d}");
        t += 0.005;
    }
}

// ---- Singing with a bank ----

/// A bank: "- あ" loud, "a あ" (the same vowel, a VCV-style join) 9 dB
/// quieter, "a い" at the loud level, "か" and "br" (a breath) when asked.
fn bank(name: &str, breath: bool) -> (PathBuf, Voicebank) {
    let root = common::temp_dir(name);
    let dir = root.join("Synth");
    std::fs::create_dir_all(&dir).unwrap();
    let mut oto = String::new();
    let mut loud = common::silence(0.1);
    loud.extend(common::vowel(220.0, 0.8, 'a'));
    common::write_wav(&dir.join("a.wav"), &loud, FS);
    oto.push_str("a.wav=- あ,80,80,-850,20,0\n");
    let quiet: Vec<f32> = common::vowel(220.0, 0.8, 'a')
        .iter()
        .map(|v| v * 0.355)
        .collect();
    common::write_wav(&dir.join("aa.wav"), &quiet, FS);
    oto.push_str("aa.wav=a あ,0,100,-780,60,40\n");
    let mut ai = common::vowel(220.0, 0.15, 'a');
    ai.extend(common::vowel(220.0, 0.7, 'i'));
    common::write_wav(&dir.join("ai.wav"), &ai, FS);
    oto.push_str("ai.wav=a い,0,200,-830,150,40\n");
    let mut ka = common::silence(0.1);
    ka.extend(common::noise(0.08, 0.2, 7));
    ka.extend(common::vowel(220.0, 0.6, 'a'));
    common::write_wav(&dir.join("ka.wav"), &ka, FS);
    oto.push_str("ka.wav=- か,80,160,-660,100,10\n");
    if breath {
        let br = common::noise(0.5, 0.15, 3);
        common::write_wav(&dir.join("br.wav"), &br, FS);
        oto.push_str("br.wav=br,0,100,0,0,0\n");
    }
    std::fs::write(dir.join("oto.ini"), oto).unwrap();
    std::fs::write(dir.join("character.txt"), "name=Synth\n").unwrap();
    let b = Voicebank::load(&dir).unwrap();
    (root, b)
}

fn phrase(list: &[(f64, f64, f32, &str)]) -> Phrase {
    Phrase {
        voice: "utau/Synth".into(),
        language: "ja".into(),
        notes: list
            .iter()
            .map(|&(s, d, p, l)| PhraseNote::new(s, d, p, l))
            .collect(),
        curves: BTreeMap::new(),
        extra_curves: BTreeMap::new(),
        settings: BTreeMap::new(),
        state: None,
        start: 0.0,
        note_ids: Vec::new(),
    }
}

fn sing(b: &Voicebank, p: &Phrase) -> Sung {
    let cache = AnalysisCache::new(None, 64 << 20);
    sing_with(b, &cache, p, Quality::Final, 1, &|| false).unwrap()
}

fn span(s: &Sung, a: f64, b: f64) -> &[f32] {
    let i = (((a - s.start) * s.sample_rate as f64).max(0.0) as usize).min(s.samples.len());
    let j = (((b - s.start) * s.sample_rate as f64).max(0.0) as usize).min(s.samples.len());
    &s.samples[i..j]
}

fn db(x: &[f32]) -> f32 {
    20.0 * rms(x).max(1e-9).log10()
}

#[test]
fn the_expression_setting_is_sung_and_can_be_turned_off() {
    let (_root, b) = bank("natural_expression", false);
    let mut p = phrase(&[(0.0, 2.0, 57.0, "[- あ]")]);
    let s = sing(&b, &p);
    let at = |s: &Sung, t: f64| s.pitch[((t - s.start) / s.pitch_step) as usize];
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    let mut t = 1.0;
    while t < 1.8 {
        let c = cents(at(&s, t), midi_hz(57.0));
        lo = lo.min(c);
        hi = hi.max(c);
        t += 0.005;
    }
    assert!(hi - lo > 30.0, "{lo} {hi}");
    // Still in tune.
    let f = measure_f0(&s.samples, FS, 0.3 - s.start, 1.9 - s.start);
    assert!(cents(f, midi_hz(57.0)).abs() < 15.0, "{f}");
    p.settings.insert(SETTING_EXPRESSION.into(), 0.0);
    let s = sing(&b, &p);
    let mut t = 0.1;
    while t < 1.9 {
        assert!(cents(at(&s, t), midi_hz(57.0)).abs() < 1.0);
        t += 0.05;
    }
}

#[test]
fn a_join_between_two_recordings_does_not_jump_in_level() {
    let (_root, b) = bank("natural_join", false);
    let mut p = phrase(&[(0.0, 0.5, 57.0, "[- あ]"), (0.5, 0.8, 57.0, "[a あ]")]);
    p.settings.insert(SETTING_EXPRESSION.into(), 0.0);
    let s = sing(&b, &p);
    // 20 ms windows across the join: no step bigger than 2 dB, although
    // the second recording is 9 dB quieter.
    let mut prev = db(span(&s, 0.2, 0.22));
    let mut t = 0.22;
    let mut worst = 0.0f32;
    while t < 1.0 {
        let d = db(span(&s, t, t + 0.02));
        worst = worst.max((d - prev).abs());
        prev = d;
        t += 0.02;
    }
    assert!(worst < 2.0, "step {worst} dB");
    // Away from the join each recording is heard at its own level.
    let first = db(span(&s, 0.1, 0.2));
    let second = db(span(&s, 1.0, 1.2));
    assert!((first - second - 9.0).abs() < 2.0, "{first} {second}");
}

#[test]
fn a_breath_is_taken_before_a_phrase_when_the_bank_has_one() {
    let (_root, b) = bank("natural_breath", true);
    let p = phrase(&[(0.0, 0.6, 57.0, "[- か]")]);
    let s = sing(&b, &p);
    assert!(s.start < -0.3, "{}", s.start);
    let breath = rms(span(&s, -0.3, -0.15));
    assert!(breath > 0.003, "{breath}");
    // Quieter than the singing.
    assert!(breath < rms(span(&s, 0.1, 0.4)) * 0.7);
    // A breath is not a phoneme of the note.
    let aliases: Vec<&str> = s.phonemes[0]
        .phonemes
        .iter()
        .map(|p| p.symbol.as_str())
        .collect();
    assert_eq!(aliases, ["- か"]);
    // Turned off, or a bank without one: no breath.
    let mut off = p.clone();
    off.settings.insert(SETTING_BREATH.into(), 0.0);
    let s = sing(&b, &off);
    assert!(s.start > -0.2, "{}", s.start);
    let (_root2, plain) = bank("natural_no_breath", false);
    let s = sing(&plain, &p);
    assert!(s.start > -0.2, "{}", s.start);
}

#[test]
fn no_breath_where_the_rest_is_too_short() {
    let (_root, b) = bank("natural_breath_short", true);
    let mut p = phrase(&[(0.0, 0.5, 57.0, "[- か]"), (0.75, 0.5, 57.0, "[- か]")]);
    p.settings.insert(SETTING_EXPRESSION.into(), 0.0);
    let s = sing(&b, &p);
    // Only the phrase's start breathes; 0.25 s between notes is too short.
    let gap = rms(span(&s, 0.54, 0.64));
    assert!(gap < 0.003, "{gap}");
}

fn frames_for(b: &Voicebank, p: &Phrase, breath: f64) -> (fontelle_utau::world::Frames, f64) {
    let cache = AnalysisCache::new(None, 64 << 20);
    let kind = phonemize::detect(b);
    let plan = phonemize::plan(b, p, kind);
    let sources: Vec<(PathBuf, UnitSource)> = b
        .entries
        .iter()
        .map(|e| {
            let a = cache.get(&e.file, &cache::options_for(&e.file)).unwrap();
            (
                e.file.clone(),
                UnitSource {
                    analysis: a,
                    median_f0: 220.0,
                },
            )
        })
        .collect();
    let find = |f: &Path| sources.iter().find(|(p, _)| p == f).map(|(_, s)| s);
    let file_ms = |f: &Path| find(f).map(|s| (s.analysis.frames() - 1) as f64 * 5.0);
    let placed = render::place(b, &plan, p, &file_ms);
    let srcs: Vec<&UnitSource> = placed
        .iter()
        .map(|u| find(&u.entry.file).unwrap())
        .collect();
    let voicing = Voicing {
        phrase: p,
        modulation: 0.0,
        expression: 0.0,
        breath,
    };
    render::assemble(&placed, &srcs, &voicing, FS, 2048)
}

#[test]
fn a_phrase_ends_on_an_aspirated_release() {
    let (_root, b) = bank("natural_release", false);
    let p = phrase(&[(0.0, 0.6, 57.0, "[- あ]")]);
    let mean_ap = |f: &fontelle_utau::world::Frames, t0: f64, t: f64| {
        let j = ((t - t0) / 0.005) as usize;
        let bins = f.fft_size / 2 + 1;
        let lim = bins * 4000 / (FS as usize / 2);
        f.ap[j * bins..j * bins + lim].iter().sum::<f32>() / lim as f32
    };
    let (dry, t0) = frames_for(&b, &p, 0.0);
    let (wet, t1) = frames_for(&b, &p, 1.0);
    // Mid-note: the same.
    assert!((mean_ap(&dry, t0, 0.3) - mean_ap(&wet, t1, 0.3)).abs() < 0.02);
    // In the release: breathier.
    let end = 0.6 - 0.006;
    assert!(
        mean_ap(&wet, t1, end) > mean_ap(&dry, t0, end) + 0.1,
        "{} {}",
        mean_ap(&wet, t1, end),
        mean_ap(&dry, t0, end)
    );
}
