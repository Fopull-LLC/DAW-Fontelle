//! What a note's five properties may hold — and, for fine pitch, what that
//! range has to be for the lane drawing it to be worth having.
//!
//! `NoteProperty::range` is a document fact rather than a view one (it is the
//! clamp `set` applies), but it is also the whole of what the roll's property
//! lane maps onto a few dozen pixels. A range nobody can aim inside is a range
//! that makes the property unusable, which is a document problem wearing a UI
//! costume.

use fontelle_model::{Note, NoteProperty};

fn blank() -> Note {
    Note {
        start: 0,
        length: 96,
        key: 60,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
    }
}

#[test]
fn fine_pitch_spans_an_octave_each_way() {
    // Not the ±8192 of a MIDI pitch bend, which is the number this started
    // as: read as the cents it is documented in, that is ±81 semitones, and a
    // lane fifty pixels tall would move a note by a minor third per pixel.
    // An octave each way is the widest that leaves a *cent* aimable-at.
    assert_eq!(NoteProperty::FinePitch.range(), (-1_200, 1_200));
}

#[test]
fn a_property_dragged_past_its_end_gets_the_end() {
    let mut note = blank();
    NoteProperty::FinePitch.set(&mut note, 9_000);
    assert_eq!(note.fine_pitch, 1_200);
    NoteProperty::FinePitch.set(&mut note, -9_000);
    assert_eq!(note.fine_pitch, -1_200);
}

#[test]
fn every_property_round_trips_through_get_and_set() {
    // One value each, all different, so a `get` reading the wrong field is a
    // failure rather than a coincidence.
    let mut note = blank();
    let written = [
        (NoteProperty::Velocity, 33),
        (NoteProperty::Pan, -44),
        (NoteProperty::FinePitch, 555),
        (NoteProperty::Release, 66),
        (NoteProperty::ModX, 77),
        (NoteProperty::ModY, 88),
    ];
    for (property, value) in written {
        property.set(&mut note, value);
    }
    for (property, value) in written {
        assert_eq!(property.get(&note), value, "{}", property.label());
    }
}

#[test]
fn the_defaults_every_note_carries_are_inside_every_range() {
    // The compatibility rule, from the document's end: a blank note is a legal
    // note, so opening one and saving it back cannot change it.
    let note = blank();
    for property in [
        NoteProperty::Velocity,
        NoteProperty::Pan,
        NoteProperty::FinePitch,
        NoteProperty::Release,
        NoteProperty::ModX,
        NoteProperty::ModY,
    ] {
        let (min, max) = property.range();
        let value = property.get(&note);
        assert!(
            value >= min && value <= max,
            "{} defaults to {value}, outside {min}..={max}",
            property.label()
        );
    }
}
