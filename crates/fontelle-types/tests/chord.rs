//! Smart chords: the chord model the piano roll's strip is built on.
//!
//! Ty, 2026-10-18: chords that sound good on the note just placed for a
//! beginner, and exactly the chord in their head, fast, for somebody who
//! knows the theory. So the same model names a chord (`Am7`), says what it
//! does in the key (`vi7`), lists the chords a note could carry, voices
//! them around the note without moving it, and reads a typed name.

use fontelle_types::KeyScale;
use fontelle_types::chord::{
    Chord, Extension, Inversion, Quality, Suggest, Voicing, parse_chord, roman, suggestions, voice,
};

const C: u8 = 0;
const D: u8 = 2;
const E: u8 = 4;
const F: u8 = 5;
const G: u8 = 7;
const A: u8 = 9;
const B: u8 = 11;

fn c_major() -> KeyScale {
    KeyScale::new(C, "major")
}

fn chord(root: u8, quality: Quality) -> Chord {
    Chord::new(root, quality)
}

fn labels(list: &[fontelle_types::chord::Suggestion]) -> Vec<String> {
    list.iter().map(|s| s.chord.label()).collect()
}

#[test]
fn chords_are_named_the_way_people_write_them() {
    assert_eq!(chord(A, Quality::Minor).label(), "Am");
    assert_eq!(chord(F, Quality::Major7).label(), "Fmaj7");
    assert_eq!(chord(G, Quality::Dominant7).label(), "G7");
    assert_eq!(chord(B, Quality::HalfDiminished7).label(), "Bm7b5");
    assert_eq!(chord(D, Quality::Sus4).label(), "Dsus4");
    assert_eq!(chord(C, Quality::Add9).label(), "Cadd9");
    let slash = Chord {
        bass: Some(E),
        ..chord(C, Quality::Major)
    };
    assert_eq!(slash.label(), "C/E");
}

#[test]
fn a_chords_tones_are_its_intervals_above_the_root() {
    assert_eq!(chord(A, Quality::Minor).pitch_classes(), vec![A, C, E]);
    assert_eq!(
        chord(G, Quality::Dominant7).pitch_classes(),
        vec![G, B, D, F]
    );
    assert_eq!(
        chord(C, Quality::Major9).pitch_classes(),
        vec![C, E, G, B, D]
    );
}

/// What the chord does in the key: upper case for major, lower for minor,
/// ° for diminished, and an accidental for a chord borrowed from outside.
#[test]
fn roman_numerals_say_what_a_chord_does_in_the_key() {
    let key = c_major();
    assert_eq!(roman(&key, &chord(C, Quality::Major)).as_deref(), Some("I"));
    assert_eq!(
        roman(&key, &chord(A, Quality::Minor)).as_deref(),
        Some("vi")
    );
    assert_eq!(
        roman(&key, &chord(D, Quality::Minor7)).as_deref(),
        Some("ii7")
    );
    assert_eq!(
        roman(&key, &chord(G, Quality::Dominant7)).as_deref(),
        Some("V7")
    );
    assert_eq!(
        roman(&key, &chord(B, Quality::Diminished)).as_deref(),
        Some("vii°")
    );
    assert_eq!(
        roman(&key, &chord(B, Quality::HalfDiminished7)).as_deref(),
        Some("viiø7")
    );
    assert_eq!(
        roman(&key, &chord(F, Quality::Major7)).as_deref(),
        Some("IVmaj7")
    );
    // Borrowed: the flat seven and the minor four of a major key.
    assert_eq!(
        roman(&key, &chord(10, Quality::Major)).as_deref(),
        Some("bVII")
    );
    assert_eq!(
        roman(&key, &chord(F, Quality::Minor)).as_deref(),
        Some("iv")
    );
}

/// In C major, a note on A, rooted: the chord built on it from the scale,
/// simplest first, every one of them in the key.
#[test]
fn rooted_suggestions_are_the_scales_own_chords_on_the_note() {
    let list = suggestions(Some(&c_major()), A, Suggest::Rooted, Extension::Triad, true);
    assert_eq!(list[0].chord, chord(A, Quality::Minor));
    assert_eq!(list[0].roman.as_deref(), Some("vi"));
    assert!(list.iter().all(|s| !s.borrowed), "{:?}", labels(&list));
    assert!(list.iter().all(|s| s.chord.root == A));
    assert!(
        labels(&list).contains(&"Am7".to_string()),
        "{:?}",
        labels(&list)
    );
}

/// Containing: every chord of the key the note belongs to — what
/// harmonising a melody is — the one it roots first.
#[test]
fn containing_suggestions_are_every_chord_of_the_key_with_the_note_in_it() {
    let list = suggestions(
        Some(&c_major()),
        A,
        Suggest::Containing,
        Extension::Triad,
        true,
    );
    let names = labels(&list);
    assert_eq!(names[0], "Am");
    for name in ["F", "Dm"] {
        assert!(names.contains(&name.to_string()), "{names:?}");
    }
    assert!(list.iter().all(|s| s.chord.pitch_classes().contains(&A)));
    assert!(!names.contains(&"G".to_string()), "G has no A: {names:?}");
}

/// The lock off, the strip offers chords from outside the key too — marked,
/// so going out of key is something seen.
#[test]
fn with_the_lock_off_borrowed_chords_are_offered_and_marked() {
    let list = suggestions(
        Some(&c_major()),
        F,
        Suggest::Containing,
        Extension::Triad,
        false,
    );
    let fm = list
        .iter()
        .find(|s| s.chord == chord(F, Quality::Minor))
        .expect("the minor four, borrowed");
    assert!(fm.borrowed);
    assert!(list.iter().any(|s| !s.borrowed));
    // The lock on, nothing borrowed.
    let locked = suggestions(
        Some(&c_major()),
        F,
        Suggest::Containing,
        Extension::Triad,
        true,
    );
    assert!(locked.iter().all(|s| !s.borrowed));
}

#[test]
fn the_extension_reaches_every_chip() {
    let list = suggestions(
        Some(&c_major()),
        G,
        Suggest::Rooted,
        Extension::Seventh,
        true,
    );
    assert_eq!(list[0].chord, chord(G, Quality::Dominant7));
    let ninths = suggestions(Some(&c_major()), C, Suggest::Rooted, Extension::Ninth, true);
    assert_eq!(ninths[0].chord, chord(C, Quality::Major9));
}

/// No scale chosen: the common chords on the note.
#[test]
fn without_a_scale_the_common_chords_on_the_note() {
    let list = suggestions(None, D, Suggest::Rooted, Extension::Triad, true);
    let names = labels(&list);
    assert_eq!(&names[..2], &["D", "Dm"]);
    assert!(list.iter().all(|s| s.roman.is_none()));
}

/// A scale that is not seven notes has no thirds to stack: its chords are
/// those whose every note is in it, not numbered.
#[test]
fn a_pentatonic_offers_chords_wholly_inside_it() {
    let key = KeyScale::new(A, "minor-pentatonic");
    let list = suggestions(Some(&key), A, Suggest::Rooted, Extension::Triad, true);
    assert!(!list.is_empty());
    let mask = key.mask().unwrap();
    assert!(
        list.iter().all(|s| s.chord.mask() & !mask == 0),
        "{:?}",
        labels(&list)
    );
}

/// The note placed never moves: the chord is built around it.
#[test]
fn a_chord_is_voiced_around_the_note_without_moving_it() {
    let am = chord(A, Quality::Minor);
    let keys = voice(&am, 69, Voicing::Close, Inversion::Root, None);
    assert_eq!(keys, vec![69, 72, 76]);
    // Containing: F over a placed A — the A stays, F and C go around it.
    let f = chord(F, Quality::Major);
    let keys = voice(&f, 69, Voicing::Close, Inversion::Root, None);
    assert!(keys.contains(&69), "{keys:?}");
    assert_eq!(
        keys.iter().map(|k| k % 12).collect::<Vec<_>>(),
        vec![F, A, C]
    );
    // First inversion: the third at the bottom.
    let c = chord(C, Quality::Major);
    let keys = voice(&c, 60, Voicing::Close, Inversion::First, None);
    assert!(keys.contains(&60));
    assert_eq!(keys[0] % 12, E);
}

#[test]
fn voicings_spread_the_same_tones_differently() {
    let cmaj7 = chord(C, Quality::Major7);
    let close = voice(&cmaj7, 60, Voicing::Close, Inversion::Root, None);
    for voicing in [Voicing::Open, Voicing::Drop2, Voicing::Spread] {
        let keys = voice(&cmaj7, 60, voicing, Inversion::Root, None);
        assert!(keys.contains(&60), "{voicing:?} moved the note: {keys:?}");
        let mut a: Vec<u8> = keys.iter().map(|k| k % 12).collect();
        let mut b: Vec<u8> = close.iter().map(|k| k % 12).collect();
        a.sort();
        b.sort();
        assert_eq!(a, b, "{voicing:?} changed the tones");
        let span = keys.last().unwrap() - keys.first().unwrap();
        assert!(
            span > close.last().unwrap() - close.first().unwrap(),
            "{voicing:?} is not wider"
        );
    }
}

/// Nearest: the inversion that moves least from the chord before — voice
/// leading done for you.
#[test]
fn the_nearest_inversion_moves_least_from_the_chord_before() {
    let before = [60, 64, 67]; // C
    let f = chord(F, Quality::Major);
    let keys = voice(&f, 65, Voicing::Close, Inversion::Nearest, Some(&before));
    assert!(keys.contains(&65));
    // C E G to C F A: two voices move a step, one stays.
    assert_eq!(keys, vec![60, 65, 69]);
}

#[test]
fn a_typed_name_is_read() {
    assert_eq!(parse_chord("Am", None), Some(chord(A, Quality::Minor)));
    assert_eq!(
        parse_chord("F#m7b5", None),
        Some(chord(6, Quality::HalfDiminished7))
    );
    assert_eq!(parse_chord("Bbsus4", None), Some(chord(10, Quality::Sus4)));
    assert_eq!(parse_chord("cmaj9", None), Some(chord(C, Quality::Major9)));
    assert_eq!(
        parse_chord("Cmaj7/E", None),
        Some(Chord {
            bass: Some(E),
            ..chord(C, Quality::Major7)
        })
    );
    assert_eq!(parse_chord("G7", None), Some(chord(G, Quality::Dominant7)));
    assert_eq!(
        parse_chord("Edim", None),
        Some(chord(E, Quality::Diminished))
    );
    assert_eq!(parse_chord("not a chord", None), None);
    // In a key, a numeral is a chord too.
    let key = c_major();
    assert_eq!(
        parse_chord("ii7", Some(&key)),
        Some(chord(D, Quality::Minor7))
    );
    assert_eq!(parse_chord("V", Some(&key)), Some(chord(G, Quality::Major)));
    assert_eq!(
        parse_chord("bVII", Some(&key)),
        Some(chord(10, Quality::Major))
    );
    // Every label reads back as itself.
    for quality in Quality::ALL {
        let c = chord(D, quality);
        assert_eq!(parse_chord(&c.label(), None), Some(c), "{}", c.label());
    }
}
