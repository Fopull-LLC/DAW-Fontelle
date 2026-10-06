//! A key as plain text: what Copy scale puts on the clipboard and what Paste
//! scale reads back.
//!
//! Ty: a "Copy scale" / "Paste scale" between the piano roll's scale chooser
//! and the pitch corrector's scale, and **plain text** on the clipboard — "A
//! minor", "C# major", "D dorian" — so it is useful in a browser too. Paste
//! reads it leniently: *A min*, *Am*, *A minor*, *a natural minor*.

use fontelle_types::{KeyScale, TuneScale, parse_scale, scale_notes, scale_text};

fn key(root: u8, id: &str) -> KeyScale {
    KeyScale::new(root, id)
}

// ------------------------------------------------------------ the writing

#[test]
fn a_key_is_written_the_way_people_say_it() {
    assert_eq!(scale_text(&key(9, "natural-minor")), "A minor");
    assert_eq!(scale_text(&key(1, "major")), "C# major");
    assert_eq!(scale_text(&key(2, "dorian")), "D dorian");
    assert_eq!(
        scale_text(&key(6, "phrygian-dominant")),
        "F# phrygian dominant"
    );
    assert_eq!(
        scale_text(&key(0, "minor-pentatonic")),
        "C minor pentatonic"
    );
}

#[test]
fn everything_written_reads_back_as_itself() {
    for scale in fontelle_types::SCALES {
        for root in 0..12 {
            let written = scale_text(&key(root, scale.id));
            assert_eq!(
                parse_scale(&written),
                Some(key(root, scale.id)),
                "{written:?} did not read back"
            );
        }
    }
}

// ------------------------------------------------------------ the reading

#[test]
fn a_minor_is_a_minor_however_it_is_typed() {
    for text in [
        "A minor",
        "A min",
        "Am",
        "am",
        "a minor",
        "a natural minor",
        "A Natural Minor",
        "A aeolian",
        "  A   minor \n",
        "A-minor",
        "A natural-minor",
    ] {
        assert_eq!(parse_scale(text), Some(key(9, "natural-minor")), "{text:?}");
    }
}

#[test]
fn a_bare_root_or_maj_is_major() {
    for text in ["C", "C major", "C maj", "CM", "Cmaj", "c ionian"] {
        assert_eq!(parse_scale(text), Some(key(0, "major")), "{text:?}");
    }
}

#[test]
fn sharps_and_flats_are_read_either_way() {
    assert_eq!(parse_scale("C# major"), Some(key(1, "major")));
    assert_eq!(parse_scale("Db major"), Some(key(1, "major")));
    assert_eq!(parse_scale("D\u{266d} major"), Some(key(1, "major")));
    assert_eq!(parse_scale("F\u{266f}m"), Some(key(6, "natural-minor")));
    assert_eq!(parse_scale("Bbm"), Some(key(10, "natural-minor")));
    assert_eq!(parse_scale("Cb major"), Some(key(11, "major")));
    assert_eq!(parse_scale("E# minor"), Some(key(5, "natural-minor")));
}

#[test]
fn modes_and_the_longer_names_are_found_by_name_or_alias() {
    assert_eq!(parse_scale("D dorian"), Some(key(2, "dorian")));
    assert_eq!(parse_scale("E phrygian"), Some(key(4, "phrygian")));
    assert_eq!(
        parse_scale("A harmonic minor"),
        Some(key(9, "harmonic-minor"))
    );
    assert_eq!(parse_scale("A harm minor"), Some(key(9, "harmonic-minor")));
    assert_eq!(parse_scale("C mel minor"), Some(key(0, "melodic-minor")));
    assert_eq!(
        parse_scale("A minor pentatonic"),
        Some(key(9, "minor-pentatonic"))
    );
    assert_eq!(parse_scale("A min pent"), Some(key(9, "minor-pentatonic")));
    assert_eq!(
        parse_scale("E spanish gypsy"),
        Some(key(4, "phrygian-dominant"))
    );
    assert_eq!(parse_scale("G blues"), Some(key(7, "blues")));
}

#[test]
fn what_is_not_a_scale_is_not_read_as_one() {
    for text in ["", "hello", "H minor", "A flurble", "123", "minor"] {
        assert_eq!(parse_scale(text), None, "{text:?}");
    }
}

#[test]
fn a_list_of_notes_reads_as_the_scale_it_spells() {
    assert_eq!(
        parse_scale("A B C D E F G"),
        Some(key(9, "natural-minor")),
        "the first note is the root"
    );
    assert_eq!(parse_scale("C, D, E, F, G, A, B"), Some(key(0, "major")));
    assert_eq!(parse_scale("C D E"), None, "no scale is just those three");
}

// ------------------------------------------------------- the notes in one

#[test]
fn the_notes_of_a_key_are_listed_from_its_root() {
    assert_eq!(
        scale_notes(&key(9, "natural-minor")),
        ["A", "B", "C", "D", "E", "F", "G"]
    );
    assert_eq!(
        scale_notes(&key(2, "major")),
        ["D", "E", "F#", "G", "A", "B", "C#"]
    );
    assert!(scale_notes(&key(0, "no-such-scale")).is_empty());
}

// ----------------------------------------------- the corrector's own list

#[test]
fn the_correctors_scales_are_the_catalogues_by_id() {
    for scale in TuneScale::ALL {
        match scale.scale_id() {
            Some(id) => {
                let named = fontelle_types::scale(id).expect("in the catalogue");
                assert_eq!(
                    named.mask(0),
                    scale.mask(0),
                    "{scale:?} and {id} are not the same notes"
                );
                assert_eq!(TuneScale::from_scale_id(id), Some(scale));
            }
            None => assert_eq!(scale, TuneScale::Custom),
        }
    }
    assert_eq!(TuneScale::from_scale_id("hirajoshi"), None);
}
