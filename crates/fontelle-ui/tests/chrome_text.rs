//! The arithmetic behind the text in the chrome.
//!
//! Every one of these was found by looking at the window rather than by
//! thinking about it, which is exactly what `docs/first-usable-plan.md` §2.5
//! predicts: the parts of a GUI that are arithmetic get tested, and the parts
//! that are not get shipped wrong. So they are arithmetic now.

use fontelle_ui::layout::Rect;
use fontelle_ui::render::{key_name, label_stride, labelled_bar, name_column};

#[test]
fn the_ruler_numbers_every_bar_when_there_is_room_for_it() {
    // The bug this exists for: the stride was computed correctly and then
    // applied as `number % stride == 1`, which for a stride of one is
    // `0 == 1` — so the common case, every bar numbered, numbered none of them.
    let stride = label_stride(480.0).expect("half a screen per bar is plenty of room");
    assert_eq!(stride, 1);
    for bar in 1..=8 {
        assert!(labelled_bar(bar, stride), "bar {bar} went unnumbered");
    }
}

#[test]
fn a_crowded_ruler_numbers_every_nth_bar_starting_at_the_first() {
    let stride = label_stride(9.0).expect("nine pixels a bar still numbers some");
    assert!(stride > 1);
    assert!(labelled_bar(1, stride), "bar 1 is always numbered");
    assert!(labelled_bar(1 + stride, stride));
    assert!(!labelled_bar(2, stride));
}

#[test]
fn a_ruler_with_no_room_at_all_numbers_nothing() {
    assert_eq!(label_stride(0.2), None);
    assert_eq!(label_stride(0.0), None);
    assert_eq!(label_stride(f32::NAN), None);
}

#[test]
fn keys_are_named_the_way_a_musician_says_them() {
    // Middle C is MIDI 60 and is called C4 — the convention every DAW and every
    // keyboard's manual uses.
    assert_eq!(key_name(60), "C4");
    assert_eq!(key_name(72), "C5");
    assert_eq!(key_name(48), "C3");
    assert_eq!(key_name(0), "C-1");
    assert_eq!(key_name(61), "C#4");
    assert_eq!(key_name(127), "G9");
}

#[test]
fn a_rows_name_stops_where_its_second_column_starts() {
    // The other bug found by looking: a soundfont called "Bread Breads
    // Distortion Guitar v2.1" and its size were drawn over each other, because
    // the name was clipped to the whole row rather than to the room left
    // beside the size.
    let row = Rect::new(10.0, 0.0, 200.0, 20.0);
    let column = name_column(row, 40.0);
    assert_eq!(column.x, row.x);
    assert!(
        column.right() <= row.right() - 40.0,
        "the name reached into the size's column"
    );
    assert!(column.width > 0.0);

    // A detail wider than the row leaves no name column rather than a negative
    // one.
    let squeezed = name_column(row, 400.0);
    assert!(squeezed.width >= 0.0);
    assert!(squeezed.is_empty());
}
