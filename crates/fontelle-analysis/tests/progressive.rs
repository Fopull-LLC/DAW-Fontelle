//! What the Analyze Musically window needs of the analysis beyond the one
//! value (plan §3.3, §3.10): notes as they are found, a way to stop, the
//! contour image the lane draws behind them, and pitch bends in cents that
//! mean what they say.

#![cfg(feature = "model")]

use fontelle_analysis::analysis::{Progress, analyse, analyse_progressive};
use fontelle_analysis::cache::{load_image, store_image};
use fontelle_analysis::spectrogram::ContourImage;
use fontelle_analysis::testsignals;
use fontelle_analysis::transcribe::basic_pitch::BasicPitch;
use std::sync::OnceLock;

fn model() -> &'static BasicPitch {
    static MODEL: OnceLock<BasicPitch> = OnceLock::new();
    MODEL.get_or_init(|| BasicPitch::load().expect("the model loads"))
}

fn median(mut values: Vec<f32>) -> f32 {
    assert!(!values.is_empty());
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

// ------------------------------------------------------------- bends ---

/// basic-pitch's contour has three bins a semitone and a key's centre is
/// the **middle** one of its three, one bin above `3 * key` — so upstream's
/// bends, measured from `3 * key`, read +1 (33 cents) on a note sung dead
/// in tune. `bend_cents` measures from the centre.
#[test]
fn an_in_tune_note_bends_by_nothing() {
    let fixture = testsignals::c_major_triad(44_100);
    let a = analyse(model(), &fixture.samples, 44_100).unwrap();
    assert_eq!(a.notes.len(), 3, "{:?}", a.notes);
    for note in &a.notes {
        let cents = note.bend_cents();
        assert!(!cents.is_empty(), "a note has its bends");
        let m = median(cents);
        assert!(m.abs() < 10.0, "key {}: bent {m} cents", note.midi);
    }
}

#[test]
fn a_sharp_note_bends_sharp() {
    // A4, 40 cents sharp, held two seconds.
    let audio = testsignals::sung_curve(44_100, 2.6, 0.3, 2.3, |_| 6940.0);
    let a = analyse(model(), &audio, 44_100).unwrap();
    let note = a
        .notes
        .iter()
        .find(|n| n.midi == 69)
        .unwrap_or_else(|| panic!("an A4 among {:?}", a.notes));
    let m = median(note.bend_cents());
    assert!((20.0..60.0).contains(&m), "bent {m} cents");
}

// ------------------------------------------------------- progressive ---

/// The sine melody four times over: 18.4 s, long enough to come in parts.
fn a_long_melody() -> Vec<f32> {
    let one = testsignals::sine_melody(22_050).samples;
    let mut out = Vec::with_capacity(one.len() * 4);
    for _ in 0..4 {
        out.extend_from_slice(&one);
    }
    out
}

#[test]
fn notes_arrive_left_to_right_before_the_end() {
    let audio = a_long_melody();
    let seconds = audio.len() as f64 / 22_050.0;
    let mut seen: Vec<(f32, usize, f64)> = Vec::new();
    let done = analyse_progressive(model(), &audio, 22_050, &mut |p: Progress<'_>| {
        let reach = p.notes.iter().map(|n| n.end).fold(0.0, f64::max);
        seen.push((p.fraction, p.notes.len(), reach));
        true
    })
    .unwrap()
    .expect("not cancelled");

    let early: Vec<_> = seen.iter().filter(|(f, n, _)| *f < 0.9 && *n > 0).collect();
    assert!(
        early.len() >= 2,
        "notes published at least twice before the end: {seen:?}"
    );
    // The first batch covers the start and not the whole: left to right.
    let (_, _, first_reach) = early[0];
    assert!(
        *first_reach < seconds * 0.75,
        "the first notes reach {first_reach} s of {seconds}"
    );
    for pair in seen.windows(2) {
        assert!(pair[1].0 >= pair[0].0, "progress only grows: {seen:?}");
    }
    assert!(seen.iter().all(|(f, _, _)| (0.0..=1.0).contains(f)));

    // And what it ends with is the analysis.
    assert_eq!(done.analysis, analyse(model(), &audio, 22_050).unwrap());
    assert_eq!(done.analysis.notes.len(), 36, "{:?}", done.analysis.notes);
}

#[test]
fn a_false_from_progress_stops_it() {
    let audio = a_long_melody();
    let mut calls = 0;
    let done = analyse_progressive(model(), &audio, 22_050, &mut |_| {
        calls += 1;
        false
    })
    .unwrap();
    assert!(done.is_none(), "cancelled");
    assert_eq!(calls, 1, "and it stopped at the first chance");
}

// ----------------------------------------------------- contour image ---

#[test]
fn the_contour_image_lights_the_row_of_the_note() {
    // A4 alone, from 0.5 s to 2.5 s.
    let audio = testsignals::sung_curve(22_050, 3.0, 0.5, 2.5, |_| 6900.0);
    let done = analyse_progressive(model(), &audio, 22_050, &mut |_| true)
        .unwrap()
        .unwrap();
    let image: &ContourImage = &done.image;
    assert!(image.columns_per_second > 10.0);
    assert_eq!(
        image.columns,
        (3.0 * image.columns_per_second).round() as usize,
        "uniform columns over the whole audio"
    );
    assert_eq!(image.data.len(), image.columns * image.rows);
    let row = image.row_of_midi(69.0).round() as usize;
    assert!((image.midi_of_row(row) - 69.0).abs() < 0.2);
    let column = (1.5 * image.columns_per_second) as usize;
    let lit = image.at(column, row);
    let off = image.at(column, image.row_of_midi(66.0).round() as usize);
    let before = image.at((0.2 * image.columns_per_second) as usize, row);
    assert!(lit > 120, "the note's row is lit: {lit}");
    assert!(
        off < lit / 3,
        "three semitones down is dark: {off} vs {lit}"
    );
    assert!(
        before < lit / 3,
        "before the note is dark: {before} vs {lit}"
    );
}

#[test]
fn the_contour_image_is_kept_beside_the_analysis() {
    let dir = std::env::temp_dir().join(format!("fontelle-analysis-image-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let image = ContourImage {
        columns: 3,
        rows: 2,
        columns_per_second: 50.0,
        data: vec![1, 2, 3, 4, 5, 6],
    };
    assert!(load_image(&dir, "k").is_none());
    store_image(&dir, "k", &image).unwrap();
    assert_eq!(load_image(&dir, "k"), Some(image));
    std::fs::write(dir.join("k.bin"), b"short").unwrap();
    assert!(load_image(&dir, "k").is_none(), "a damaged file is a miss");
    let _ = std::fs::remove_dir_all(&dir);
}
