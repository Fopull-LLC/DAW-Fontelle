//! The chord lane (plan §2.6, §4 P1).

use fontelle_analysis::chords::{ChordSpan, TimedPitch, detect_chords, melody_chords};

fn chord(start: f64, end: f64, keys: &[u8]) -> Vec<TimedPitch> {
    keys.iter()
        .map(|&midi| TimedPitch {
            start,
            end,
            midi,
            weight: 0.8,
        })
        .collect()
}

fn labels(spans: &[ChordSpan]) -> Vec<String> {
    spans
        .iter()
        .map(|s| s.chord.map(|c| c.label()).unwrap_or_else(|| "-".into()))
        .collect()
}

#[test]
fn i_vi_iv_v_reads_back() {
    let mut notes = Vec::new();
    notes.extend(chord(0.0, 2.0, &[48, 60, 64, 67])); // C
    notes.extend(chord(2.0, 4.0, &[45, 60, 64, 69])); // Am
    notes.extend(chord(4.0, 6.0, &[41, 60, 65, 69])); // F
    notes.extend(chord(6.0, 8.0, &[43, 62, 67, 71])); // G
    let spans = detect_chords(&notes, 8.0, 0.5);
    assert_eq!(labels(&spans), ["C", "Am", "F", "G"], "{spans:?}");
    for (span, at) in spans.iter().zip([0.0, 2.0, 4.0, 6.0]) {
        assert!((span.start - at).abs() < 1e-9, "{spans:?}");
        assert!(span.confidence > 0.8, "{spans:?}");
    }
}

#[test]
fn sevenths_suspensions_and_fifths_are_named() {
    let mut notes = Vec::new();
    notes.extend(chord(0.0, 1.0, &[43, 59, 62, 65, 67])); // G7
    notes.extend(chord(1.0, 2.0, &[50, 62, 67, 69])); // Dsus4
    notes.extend(chord(2.0, 3.0, &[40, 47, 52])); // E5
    notes.extend(chord(3.0, 4.0, &[45, 60, 64, 67, 69])); // Am7
    notes.extend(chord(4.0, 5.0, &[41, 64, 65, 69, 72])); // Fmaj7
    let spans = detect_chords(&notes, 5.0, 0.5);
    assert_eq!(
        labels(&spans),
        ["G7", "Dsus4", "E5", "Am7", "Fmaj7"],
        "{spans:?}"
    );
}

#[test]
fn a_melody_over_a_held_chord_is_still_the_chord() {
    let mut notes = chord(0.0, 4.0, &[45, 57, 60, 64]); // Am
    for (k, key) in [69u8, 71, 72, 74, 76, 74, 72, 71].iter().enumerate() {
        notes.push(TimedPitch {
            start: k as f64 * 0.5,
            end: k as f64 * 0.5 + 0.5,
            midi: *key,
            weight: 0.8,
        });
    }
    let spans = detect_chords(&notes, 4.0, 0.5);
    assert_eq!(labels(&spans), ["Am"], "{spans:?}");
}

#[test]
fn silence_is_no_chord() {
    let mut notes = chord(0.0, 1.0, &[48, 52, 55]);
    notes.extend(chord(2.0, 3.0, &[48, 52, 55]));
    let spans = detect_chords(&notes, 3.0, 0.5);
    assert_eq!(labels(&spans), ["C", "-", "C"], "{spans:?}");
    assert!(
        detect_chords(&[], 2.0, 0.5)
            .iter()
            .all(|s| s.chord.is_none())
    );
}

/// One voice, a note at a time, `step` seconds each.
fn line(keys: &[u8], step: f64) -> Vec<TimedPitch> {
    keys.iter()
        .enumerate()
        .map(|(k, &midi)| TimedPitch {
            start: k as f64 * step,
            end: (k + 1) as f64 * step,
            midi,
            weight: 0.8,
        })
        .collect()
}

/// The coordinator, on the real melody: *"in Melody mode the chord lane
/// names single sung notes as chords ("Csus2", "Dsus2", "E5", "G5")"*. A
/// scale sung up and down implies no chord a bar, and certainly not one a
/// note.
#[test]
fn a_monophonic_scale_line_names_no_chord_per_note() {
    let scale = [
        60u8, 62, 64, 65, 67, 69, 71, 72, 71, 69, 67, 65, 64, 62, 60, 62,
    ];
    let notes = line(&scale, 0.25);
    // Bars of two seconds (120 BPM, four beats).
    let spans = melody_chords(&notes, 4.0, 2.0);
    let named: Vec<String> = spans
        .iter()
        .filter_map(|s| s.chord.map(|c| c.label()))
        .collect();
    assert!(
        named
            .iter()
            .all(|l| !l.contains("sus") && !l.ends_with('5')),
        "{named:?}"
    );
    // Never more names than bars: a name is a bar's, not a note's.
    assert!(named.len() <= 2, "{spans:?}");
    for span in spans.iter().filter(|s| s.chord.is_some()) {
        assert!(span.end - span.start >= 2.0 - 1e-9, "{spans:?}");
    }
    // And the per-step reading this replaces does name them, which is the
    // fault: one name every half second.
    let old = detect_chords(&notes, 4.0, 0.5);
    assert!(old.iter().filter(|s| s.chord.is_some()).count() > 2);
}

/// A line that spells a chord in a bar — an arpeggio — is named, once.
#[test]
fn an_arpeggio_in_a_bar_is_named_once() {
    // C E G C, then A C E A: C, then Am.
    let notes = line(&[60, 64, 67, 72, 57, 60, 64, 69], 0.5);
    let spans = melody_chords(&notes, 4.0, 2.0);
    assert_eq!(labels(&spans), ["C", "Am"], "{spans:?}");
}

/// One held note, or two, is not a chord.
#[test]
fn a_held_note_or_an_interval_names_nothing() {
    let notes = line(&[64, 64, 64, 64], 0.5);
    assert!(
        melody_chords(&notes, 2.0, 2.0)
            .iter()
            .all(|s| s.chord.is_none())
    );
    let notes = line(&[62, 69, 62, 69], 0.5);
    assert!(
        melody_chords(&notes, 2.0, 2.0)
            .iter()
            .all(|s| s.chord.is_none())
    );
}
