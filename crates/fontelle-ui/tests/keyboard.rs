//! The keyboard down the side of the roll, and the room it needs when the
//! instrument on the channel has names to put on it.
//!
//! A drum kit's keys are worth naming — `Kick`, `Snare`, `Closed Hat` — and 56
//! pixels is enough for `C4` and nothing else. The strip therefore has two
//! widths: the one it has always had, and a wider one for a named key map.
//!
//! **The narrow one is the default and stays the default**, which is the whole
//! point. A melodic soundfont names nothing (see `fontelle-app`'s `keymap`),
//! so a normal session's roll is the roll it always was, down to the pixel.

use fontelle_ui::canvas::{
    KEYBOARD_WIDTH, KeyStyle, NAMED_KEYBOARD_WIDTH, RollView, SnapDivision, hit_test, key_row,
    keyboard_width, keyboard_width_for, roll_layout, roll_layout_with_keys, zoom_y,
};
use fontelle_ui::document::{KeyInfo, KeyMap};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn m() -> Metrics {
    Theme::dark_default().metrics
}

fn frame() -> Rect {
    Rect::new(0.0, 0.0, 900.0, 520.0)
}

fn a_map(named: bool) -> KeyMap {
    KeyMap::new(
        (0..KeyMap::KEYS)
            .map(|key| KeyInfo {
                playable: true,
                name: named.then(|| format!("hit {key}")),
            })
            .collect(),
    )
}

#[test]
fn the_default_layout_is_the_narrow_keyboard_to_the_pixel() {
    assert_eq!(
        roll_layout(frame(), &m(), 78.0),
        roll_layout_with_keys(frame(), &m(), 78.0, KEYBOARD_WIDTH),
        "adding a width parameter must not have moved anything by default"
    );
}

#[test]
fn a_named_key_map_asks_for_the_wider_strip_and_an_unnamed_one_does_not() {
    assert_eq!(keyboard_width(&a_map(true)), NAMED_KEYBOARD_WIDTH);
    assert_eq!(keyboard_width(&a_map(false)), KEYBOARD_WIDTH);
    assert_eq!(
        keyboard_width(&KeyMap::unknown()),
        KEYBOARD_WIDTH,
        "a channel with no instrument has nothing to write there"
    );
}

#[test]
fn the_wider_strip_takes_its_room_from_the_grid_and_nothing_else() {
    let narrow = roll_layout_with_keys(frame(), &m(), 78.0, KEYBOARD_WIDTH);
    let wide = roll_layout_with_keys(frame(), &m(), 78.0, NAMED_KEYBOARD_WIDTH);

    let grew = NAMED_KEYBOARD_WIDTH - KEYBOARD_WIDTH;
    assert!(grew > 0.0, "the named strip is the wider of the two");
    assert_eq!(wide.keys.width, narrow.keys.width + grew);
    assert_eq!(
        wide.grid.x,
        narrow.grid.x + grew,
        "the grid starts where the keyboard ends"
    );
    assert_eq!(
        wide.grid.right(),
        narrow.grid.right(),
        "and still ends where the panel does"
    );
    assert_eq!(wide.grid.y, narrow.grid.y, "nothing moved vertically");
    assert_eq!(
        (wide.velocity.y, wide.velocity.height),
        (narrow.velocity.y, narrow.velocity.height),
        "and the property lane is the height it was"
    );
    // It does move *across*, and it has to: the lane's label strip is the
    // keyboard's continuation below the grid, and a strip that stayed 56 wide
    // under a 132-wide keyboard would put the lane out of line with the notes
    // it describes.
    assert_eq!(
        wide.velocity_keys.width, wide.keys.width,
        "the lane's label strip stays lined up with the keyboard"
    );
    assert_eq!(wide.velocity.x, wide.grid.x, "and so does the lane itself");
}

#[test]
fn a_key_strip_wider_than_the_panel_still_leaves_a_grid() {
    let l = roll_layout_with_keys(frame(), &m(), 78.0, 5_000.0);
    assert!(
        l.keys.width <= frame().width,
        "the keyboard cannot be wider than the panel it is in"
    );
    assert!(l.grid.width >= 0.0, "and the grid is not a negative width");
}

#[test]
fn the_grid_hit_test_follows_the_keyboard_when_it_widens() {
    let v = RollView {
        scroll_tick: 0,
        top_key: 72,
        key_offset: 0.0,
        pixels_per_tick: 0.25,
        key_height: 12.0,
        snap: SnapDivision::Step,
    };
    let wide = roll_layout_with_keys(frame(), &m(), 78.0, NAMED_KEYBOARD_WIDTH);
    let notes = fontelle_model::Arena::default();

    // A point that used to be the first column of the grid is now keyboard.
    let x = KEYBOARD_WIDTH + 2.0;
    assert!(
        matches!(
            hit_test(&v, wide.grid, &notes, x, wide.grid.y + 4.0),
            fontelle_ui::canvas::RollHit::Outside
        ),
        "clicking the name strip must not draw a note at bar one"
    );
}

// ------------------------------------------------------- even key heights ---
//
// Reported from using the roll: *"right now there is inconsistant sizing on the
// notes in the piano roll... just on some basic ones just single white keys
// wont be as tall as other white keys for some reason like in this screenshot
// example the e key is smaller"*, and *"we need to figure out how to ensure the
// piano roll is always cleanly displayed with even spacing at all times"*.
//
// Two separate causes, and both are here.
//
// 1. **The rows landed on fractions of a pixel.** `key_height` is the vertical
//    zoom and every zoom multiplied it by 1.2, so two notches from 16 gave
//    23.04 and every row after the first was drawn a fraction lower than the
//    last. The rasteriser rounds, so the rows came out 23, 23, 24, 23, 24 —
//    which is exactly "some notes are taller than others", and it got worse the
//    longer you zoomed.
//
// 2. **The key strip drew a real piano.** A natural was one row and an
//    accidental was a short bar over the *left* of its own row, so the white
//    left over on the right of C#'s row read as part of C's key. C and D looked
//    nearly two rows tall; E and B, which have no accidental above them, looked
//    exactly one. That is what a side view of a keyboard looks like and it is
//    not what an evenly spaced grid looks like.

fn view_at(key_height: f32) -> RollView {
    RollView {
        key_height,
        ..RollView::default()
    }
}

#[test]
fn zooming_leaves_rows_a_whole_number_of_pixels_tall() {
    let grid = Rect::new(0.0, 30.0, 800.0, 400.0);
    let mut view = view_at(16.0);
    for _ in 0..6 {
        zoom_y(&mut view, grid, grid.y, 1.2);
        assert_eq!(
            view.key_height,
            view.key_height.round(),
            "a row height with a fraction in it draws some rows a pixel taller \
             than others"
        );
    }
    for _ in 0..20 {
        zoom_y(&mut view, grid, grid.y, 1.0 / 1.2);
        assert_eq!(view.key_height, view.key_height.round());
    }
}

#[test]
fn zooming_out_still_gets_smaller_and_zooming_in_still_gets_bigger() {
    // The rounding must not swallow the zoom: a step of 1.2 on a five-pixel
    // row rounds to six, not back to five, or the button stops working at the
    // bottom of the range.
    let grid = Rect::new(0.0, 30.0, 800.0, 400.0);
    let mut view = view_at(6.0);
    let before = view.key_height;
    zoom_y(&mut view, grid, grid.y, 1.2);
    assert!(view.key_height > before, "zooming in did nothing");
    let before = view.key_height;
    zoom_y(&mut view, grid, grid.y, 1.0 / 1.2);
    assert!(view.key_height < before, "zooming out did nothing");
}

#[test]
fn every_row_is_the_same_height_and_they_tile_without_gaps() {
    // What the renderer draws each key in. Snapped to whole pixels here rather
    // than in the drawing code, so the keyboard, the grid rows and the notes
    // in them are all measured the same way and cannot disagree by a pixel.
    let grid = Rect::new(0.0, 30.5, 800.0, 400.0);
    let view = view_at(17.0);
    let rows: Vec<Rect> = (60..72).map(|key| key_row(&view, grid, key)).collect();
    let first = rows[0].height;
    for row in &rows {
        assert_eq!(row.height, first, "the rows are not all the same height");
        assert_eq!(row.y, row.y.round(), "a row started on half a pixel");
    }
    for pair in rows.windows(2) {
        // Keys go *up* the screen, so the next key up ends where this one
        // begins.
        assert_eq!(pair[1].bottom(), pair[0].y, "the rows do not tile");
    }
}

// ---------------------------------------------------- piano or plain list ---
//
// Also asked for: *"there should also be view options to switch between a piano
// visual view or just a plain list of names which would configure how the piano
// roll displays for that instrument... this would be useful for drums since like
// right now if a drum sound if on a black key i cant even read it."*

#[test]
fn the_strip_is_wide_enough_to_read_names_in_the_list_view() {
    // A list of names on 56 pixels is a list of the first four letters.
    let melodic = KeyMap::unknown();
    assert_eq!(
        keyboard_width_for(&melodic, KeyStyle::Piano),
        KEYBOARD_WIDTH
    );
    assert_eq!(
        keyboard_width_for(&melodic, KeyStyle::Names),
        NAMED_KEYBOARD_WIDTH,
        "the list view is what the wider strip is for"
    );
}

#[test]
fn a_named_kit_asks_for_the_wide_strip_in_either_view() {
    // The rule that was already here: an instrument whose keys have names of
    // their own gets room for them whichever way the strip is drawn.
    let kit = a_map(true);
    assert_eq!(
        keyboard_width_for(&kit, KeyStyle::Piano),
        NAMED_KEYBOARD_WIDTH
    );
    assert_eq!(
        keyboard_width_for(&kit, KeyStyle::Names),
        NAMED_KEYBOARD_WIDTH
    );
}

#[test]
fn the_default_is_the_piano_it_always_was() {
    // Down to the pixel, for the reason this file's header gives.
    let melodic = KeyMap::unknown();
    assert_eq!(KeyStyle::default(), KeyStyle::Piano);
    assert_eq!(
        keyboard_width(&melodic),
        keyboard_width_for(&melodic, KeyStyle::default())
    );
}

#[test]
fn the_view_says_which_one_it_is_so_a_button_can_say_it_too() {
    assert_eq!(KeyStyle::Piano.label(), "keys");
    assert_eq!(KeyStyle::Names.label(), "list");
    assert_eq!(KeyStyle::Piano.next(), KeyStyle::Names);
    assert_eq!(KeyStyle::Names.next(), KeyStyle::Piano);
}
