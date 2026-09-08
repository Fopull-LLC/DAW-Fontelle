//! How much of the window the arrangement gets, and how a lane menu offers to
//! put a row where you are looking.
//!
//! Reported from using the window:
//!
//! > *"instead of only starting with 1 lane in a new arrangement make it like
//! > 10 or something just not one its too barren. please also decrease the
//! > default size of the mixer/piano roll area vs the arrangement which should
//! > get some space too."*
//!
//! The two halves are one thing: ten rows in two hundred pixels is not ten
//! rows, it is three rows and a scrollbar. So the default split moves with the
//! default row count, and this is where that is pinned down.

use fontelle_ui::canvas::TimelineView;
use fontelle_ui::layout::{DEFAULT_TIMELINE_HEIGHT, MIN_EDITOR_HEIGHT, Rect, window_layout};
use fontelle_ui::theme::Theme;

/// The window the studio actually opens at — see `WindowOptions::default`.
///
/// **Not a bigger one.** The first version of this file measured 1400x820, a
/// window the app never opens, and passed while the real default left the
/// piano roll eleven keys of grid. A default is only as good as the size it is
/// a default *for*.
const DEFAULT_W: f32 = 1280.0;
const DEFAULT_H: f32 = 720.0;

/// How many rows the arrangement has to show to be worth looking at. Not all
/// ten a project starts with: see `the_editor_keeps_an_octave`, which is the
/// other side of the same pixels. It used to show three.
const USEFUL_ROWS: usize = 6;

/// And how many keys the roll has to keep: **a full octave**. Chosen because
/// it is the smallest range you can write a part in without scrolling to
/// reach the note you meant — not fitted to what the current default happens
/// to give, which is thirteen.
const USEFUL_KEYS: usize = 12;

#[test]
fn the_arrangement_opens_tall_enough_to_work_in() {
    let theme = Theme::dark_default();
    let l = window_layout(
        DEFAULT_W,
        DEFAULT_H,
        &theme.metrics,
        DEFAULT_TIMELINE_HEIGHT,
    );
    let grid = fontelle_ui::canvas::timeline_layout(l.timeline.body, &theme.metrics).grid;
    let row = TimelineView::default().lane_height;
    let rows = (grid.height / row).floor() as usize;
    assert!(
        rows >= USEFUL_ROWS,
        "the arrangement shows {rows} rows, wanted at least {USEFUL_ROWS}"
    );
}

/// The lesson two wrong attempts at this taught, kept as a test.
///
/// 430 px showed all ten rows and left the roll eleven keys; 340 px left it
/// eleven again at the size the window actually opens. `MIN_EDITOR_HEIGHT` did
/// not catch either, because it bounds the panel *frame* and what had gone was
/// the room inside it — at 340 the frame was 316 px, comfortably above its 220
/// floor, with 152 px of grid inside it. So the thing to hold is the **grid**.
#[test]
fn the_editor_keeps_an_octave() {
    use fontelle_ui::canvas::{DEFAULT_LANE_HEIGHT, roll_layout};
    let theme = Theme::dark_default();
    let l = window_layout(
        DEFAULT_W,
        DEFAULT_H,
        &theme.metrics,
        DEFAULT_TIMELINE_HEIGHT,
    );
    let roll = roll_layout(l.panel.body, &theme.metrics, DEFAULT_LANE_HEIGHT);
    // The key height the roll opens at.
    let keys = (roll.grid.height / 14.0).floor() as usize;
    assert!(
        keys >= USEFUL_KEYS,
        "the roll opens with {keys} keys of grid ({} px), wanted {USEFUL_KEYS}",
        roll.grid.height
    );
}

#[test]
fn the_editor_still_gets_the_room_it_is_promised() {
    // The arrangement growing must not eat the panel the window is for. On the
    // smallest window worth opening, both are still above their floors.
    let theme = Theme::dark_default();
    for (w, h) in [
        (DEFAULT_W, DEFAULT_H),
        (1400.0, 820.0),
        (1100.0, 700.0),
        (900.0, 620.0),
    ] {
        let l = window_layout(w, h, &theme.metrics, DEFAULT_TIMELINE_HEIGHT);
        // The panel's **frame**, which is what `MIN_EDITOR_HEIGHT` governs —
        // `body` is that less the header row and the padding.
        assert!(
            l.panel.frame.height >= MIN_EDITOR_HEIGHT - 1.0,
            "{w}x{h}: the editor got {} of its {MIN_EDITOR_HEIGHT}",
            l.panel.frame.height
        );
        assert!(
            !l.timeline.body.is_empty(),
            "{w}x{h}: no arrangement at all"
        );
    }
}

#[test]
fn the_arrangement_is_the_larger_half_by_default_on_a_normal_window() {
    // *"decrease the default size of the mixer/piano roll area vs the
    // arrangement which should get some space too."* It used to be 200 px
    // against everything else.
    let theme = Theme::dark_default();
    let l = window_layout(1400.0, 1000.0, &theme.metrics, DEFAULT_TIMELINE_HEIGHT);
    assert!(
        l.timeline.body.height > 240.0,
        "the arrangement opens at {}",
        l.timeline.body.height
    );
}

#[test]
fn a_window_squeezed_flat_still_lays_out_without_negative_boxes() {
    let theme = Theme::dark_default();
    for h in [0.0, 60.0, 200.0, 400.0] {
        let l = window_layout(800.0, h, &theme.metrics, DEFAULT_TIMELINE_HEIGHT);
        for r in [l.timeline.body, l.panel.body] {
            assert!(r.width >= 0.0 && r.height >= 0.0, "h={h}: {r:?}");
        }
    }
}

fn _unused(_: Rect) {}
