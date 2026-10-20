//! The piano roll's smart-chord strip, as a layout from rectangles and a
//! hit from a point: chips of the chords a placed note could carry, just
//! above the note, and the controls that reshape every chip at once.
//!
//! Ty, 2026-10-18: chords that sound good on the note just placed; hover
//! one to hear it and see it, click to write it; a quick switch between
//! chords rooted on the note and chords containing it.

use fontelle_types::chord::{Chord, Quality, Suggestion};
use fontelle_ui::canvas::{
    ChordStripHit, chord_chip_caption, chord_strip_hit, chord_strip_layout,
    chord_strip_layout_with_room,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

const GRID: Rect = Rect {
    x: 300.0,
    y: 400.0,
    width: 900.0,
    height: 300.0,
};

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn note_at(x: f32, y: f32) -> Rect {
    Rect::new(x, y, 60.0, 16.0)
}

#[test]
fn the_strip_sits_just_above_the_note_inside_the_grid() {
    let m = Theme::dark_default().metrics;
    let note = note_at(500.0, 600.0);
    let l = chord_strip_layout(GRID, note, 6, &m);
    assert_eq!(l.chips.len(), 6);
    assert!(l.frame.bottom() <= note.y + 0.5, "it covers the note");
    assert!(note.y - l.frame.bottom() < 12.0, "it is not near the note");
    assert!(l.frame.intersection(&GRID) == l.frame, "it leaves the grid");
    for chip in &l.chips {
        assert!(chip.intersection(&l.frame) == *chip);
    }
    for (a, b) in l.chips.iter().zip(l.chips.iter().skip(1)) {
        assert!(a.intersection(b).is_empty(), "chips overlap");
    }
}

/// Near the top of the view there is no room above: it goes under.
#[test]
fn near_the_top_the_strip_goes_under_the_note() {
    let m = Theme::dark_default().metrics;
    let note = note_at(500.0, GRID.y + 4.0);
    let l = chord_strip_layout(GRID, note, 4, &m);
    assert!(
        l.frame.y >= note.bottom() - 0.5,
        "{:?} over {:?}",
        l.frame,
        note
    );
    assert!(l.frame.intersection(&GRID) == l.frame);
}

/// At the right edge it slides left rather than off the grid.
#[test]
fn at_the_edge_the_strip_slides_in() {
    let m = Theme::dark_default().metrics;
    let note = note_at(GRID.right() - 30.0, 600.0);
    let l = chord_strip_layout(GRID, note, 8, &m);
    assert!(l.frame.right() <= GRID.right() + 0.5);
    assert!(l.frame.x >= GRID.x - 0.5);
}

#[test]
fn every_part_of_the_strip_answers_a_press() {
    let m = Theme::dark_default().metrics;
    let l = chord_strip_layout(GRID, note_at(500.0, 600.0), 5, &m);
    for (i, chip) in l.chips.iter().enumerate() {
        let (x, y) = centre(*chip);
        assert_eq!(chord_strip_hit(&l, x, y), Some(ChordStripHit::Chip(i)));
    }
    let controls = [
        (l.mode, ChordStripHit::Mode),
        (l.extension, ChordStripHit::Extension),
        (l.voicing, ChordStripHit::Voicing),
        (l.inversion, ChordStripHit::Inversion),
        (l.type_it, ChordStripHit::Type),
    ];
    for (rect, hit) in controls {
        assert!(!rect.is_empty(), "{hit:?}");
        assert!(rect.intersection(&l.frame) == rect, "{hit:?} outside");
        let (x, y) = centre(rect);
        assert_eq!(chord_strip_hit(&l, x, y), Some(hit));
    }
    assert_eq!(chord_strip_hit(&l, GRID.x + 1.0, GRID.bottom() - 1.0), None);
}

/// A chip says the chord's name, and its job in the key in small type; a
/// borrowed chord is marked.
#[test]
fn a_chip_names_the_chord_and_its_job() {
    let vi = Suggestion {
        chord: Chord::new(9, Quality::Minor),
        roman: Some("vi".into()),
        borrowed: false,
    };
    assert_eq!(
        chord_chip_caption(&vi),
        ("Am".to_string(), "vi".to_string())
    );
    let borrowed = Suggestion {
        chord: Chord::new(5, Quality::Minor),
        roman: Some("iv".into()),
        borrowed: true,
    };
    let (_, small) = chord_chip_caption(&borrowed);
    assert!(small.contains("iv") && small.contains('•'), "{small}");
    let plain = Suggestion {
        chord: Chord::new(2, Quality::Major),
        roman: None,
        borrowed: false,
    };
    assert_eq!(chord_chip_caption(&plain).1, "");
}

/// Placed for the most chips it could hold, a strip by the right edge does
/// not slide when a control gives it more: only its right edge moves.
#[test]
fn a_strip_with_room_keeps_its_corner_as_it_grows() {
    let m = Theme::dark_default().metrics;
    let note = note_at(GRID.right() - 120.0, 600.0);
    let few = chord_strip_layout_with_room(GRID, note, 3, 10, &m);
    let many = chord_strip_layout_with_room(GRID, note, 10, 10, &m);
    assert_eq!((few.frame.x, few.frame.y), (many.frame.x, many.frame.y));
    assert_eq!(few.mode, many.mode);
    assert!(few.frame.width < many.frame.width);
    assert!(many.frame.right() <= GRID.right() + 0.5);
}
