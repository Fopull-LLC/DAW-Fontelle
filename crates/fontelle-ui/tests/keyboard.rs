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
    KEYBOARD_WIDTH, NAMED_KEYBOARD_WIDTH, RollView, SnapDivision, hit_test, keyboard_width,
    roll_layout, roll_layout_with_keys,
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
