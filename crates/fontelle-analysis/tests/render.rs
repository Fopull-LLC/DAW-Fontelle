//! A study's edits rendered (`docs/analyze-musically-plan.md` §2.4, §3.7):
//! the whole audio back, the same length, with each edit's pitch contour
//! — the move eased in and out, the drift flattened, the vibrato scaled —
//! and nothing else changed.

use fontelle_analysis::mono::{PyinParams, pyin, segment};
use fontelle_analysis::render::render_edits;
use fontelle_analysis::resynth::Psola;
use fontelle_analysis::testsignals::{drifting_vibrato_note, sung_curve};
use fontelle_types::PitchEdit;

const SR: u32 = 44_100;

fn frames(seconds: f64) -> i64 {
    (seconds * f64::from(SR)).round() as i64
}

fn the_note(audio: &[f32]) -> fontelle_analysis::mono::MonoNote {
    segment(&pyin(audio, SR, &PyinParams::default()))
        .into_iter()
        .max_by_key(|n| n.end - n.first)
        .expect("a note")
}

fn rms(values: &[f32]) -> f32 {
    let n = values.len();
    let middle = &values[n / 5..n - n / 5];
    (middle.iter().map(|v| v * v).sum::<f32>() / middle.len() as f32).sqrt()
}

/// Whole-file length, whatever was edited, at every channel count: a clip's
/// `source_start/end`, fades and loop stay valid on the render (plan §3.7).
#[test]
fn a_render_is_the_same_length_as_the_source() {
    let mono = sung_curve(SR, 1.5, 0.1, 1.4, |_| 5700.0);
    for channels in [1usize, 2] {
        let audio: Vec<f32> = mono
            .iter()
            .flat_map(|s| std::iter::repeat_n(*s, channels))
            .collect();
        for edits in [
            vec![],
            vec![PitchEdit {
                shift_cents: 120.0,
                ..PitchEdit::none((frames(0.1), frames(1.4)))
            }],
            // An edit hanging off both ends of the audio.
            vec![PitchEdit {
                shift_cents: -80.0,
                ..PitchEdit::none((-500, frames(9.0)))
            }],
        ] {
            let rendered = render_edits(&audio, channels, SR, &edits, &Psola);
            assert_eq!(
                rendered.audio.len(),
                audio.len(),
                "{channels} channels, {edits:?}"
            );
        }
    }
}

/// F: the drift taken out. A note drifting 60 cents across its length,
/// flattened fully, drifts a fraction of that.
#[test]
fn flattening_takes_the_drift_out() {
    let audio = drifting_vibrato_note(SR);
    let edit = PitchEdit {
        flatten: 1.0,
        ..PitchEdit::none((frames(0.25), frames(2.25)))
    };
    let rendered = render_edits(&audio, 1, SR, &[edit], &Psola);
    let spread = |n: &fontelle_analysis::mono::MonoNote| {
        let k = n.drift.len();
        let middle = &n.drift[k / 5..k - k / 5];
        middle.iter().fold(f32::MIN, |m, v| m.max(*v))
            - middle.iter().fold(f32::MAX, |m, v| m.min(*v))
    };
    let (before, after) = (
        spread(&the_note(&audio)),
        spread(&the_note(&rendered.audio)),
    );
    println!("drift across the middle: {before:.1} ct before, {after:.1} ct flattened");
    assert!(before > 25.0, "{before}");
    assert!(after < before * 0.35, "{before} -> {after}");
}

/// V: the vibrato scaled — none at 0, half at 0.5.
#[test]
fn the_vibrato_is_scaled() {
    let audio = drifting_vibrato_note(SR);
    let before = rms(&the_note(&audio).vibrato);
    for (scale, low, high) in [(0.0f32, 0.0f32, 0.3f32), (0.5, 0.35, 0.65)] {
        let edit = PitchEdit {
            vibrato: scale,
            ..PitchEdit::none((frames(0.25), frames(2.25)))
        };
        let rendered = render_edits(&audio, 1, SR, &[edit], &Psola);
        let after = rms(&the_note(&rendered.audio).vibrato);
        println!("vibrato x{scale}: {before:.1} ct -> {after:.1} ct");
        assert!(
            after / before >= low && after / before <= high,
            "x{scale}: {before} -> {after}"
        );
    }
}

/// The move eases in over the glide: a long glide in leaves the note's
/// start near where it was, its middle moved the whole way.
#[test]
fn the_move_glides_in_and_out() {
    let audio = sung_curve(SR, 2.0, 0.2, 1.8, |_| 5700.0);
    let edit = PitchEdit {
        shift_cents: 200.0,
        glide_in_ms: 400.0,
        glide_out_ms: 40.0,
        ..PitchEdit::none((frames(0.2), frames(1.8)))
    };
    let rendered = render_edits(&audio, 1, SR, &[edit], &Psola);
    let track = pyin(&rendered.audio, SR, &PyinParams::default());
    let at = |t: f64| {
        let i = (t / track.hop).round() as usize;
        track.cents(i).expect("voiced")
    };
    let (early, middle) = (at(0.27), at(1.0));
    println!("glide in: {early:.0} ct 70 ms in, {middle:.0} ct in the middle");
    assert!(early < 5700.0 + 60.0, "{early}");
    assert!((middle - 5900.0).abs() < 4.0, "{middle}");
}

/// The Pitch card's GAIN: a note louder or quieter over its own span, eased
/// in and out, and the rest untouched — with no pitch move at all.
#[test]
fn a_notes_gain_is_heard_over_its_span_only() {
    let audio = sung_curve(SR, 3.0, 0.1, 2.9, |_| 5700.0);
    let span = (frames(1.0), frames(2.0));
    let edits = vec![PitchEdit {
        gain_db: -6.0206,
        ..PitchEdit::none(span)
    }];
    let rendered = render_edits(&audio, 1, SR, &edits, &Psola);
    let window = |a: f64, b: f64| (frames(a) as usize, frames(b) as usize);
    let (a, b) = window(1.2, 1.8);
    let ratio = rms(&rendered.audio[a..b]) / rms(&audio[a..b]);
    assert!((ratio - 0.5).abs() < 0.02, "{ratio}");
    let (a, b) = window(0.2, 0.9);
    assert_eq!(
        &rendered.audio[a..b],
        &audio[a..b],
        "before it, bit for bit"
    );
    let (a, b) = window(2.1, 2.8);
    assert_eq!(&rendered.audio[a..b], &audio[a..b], "after it too");
}
