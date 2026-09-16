//! Dragging the window's own seams.
//!
//! Reported from using the window: *"not all of the windows are draggable right
//! now — the left section, I can't resize it. I want to be able to make the
//! soundfonts bigger than the channels, but that's just my preference, so I
//! would want to drag it but the program doesn't let me right now. Ensure that
//! the app is customizable and free for the user to use to their preference."*
//!
//! Exactly right, and the numbers were hard-coded: the sidebar was always the
//! theme's `sidebar_width`, and the channel rack always took [`RACK_SHARE`] of
//! its height. Neither is a decision this program gets to make for somebody
//! who is looking at it all day.
//!
//! So there are two more seams, laid out the same way the arrangement's
//! divider already was — as arithmetic, with the clamps *in the arithmetic*
//! rather than in an event handler. The clamps are what these tests are mostly
//! about: a seam that can be dragged until a panel is a one-pixel sliver is a
//! seam that will be, once, by accident, and there is no way back from it
//! except quitting.

use fontelle_ui::layout::{
    DEFAULT_TIMELINE_HEIGHT, Docks, MIN_BROWSER_HEIGHT, MIN_EDITOR_HEIGHT, MIN_RACK_HEIGHT,
    MIN_ROLL_WIDTH, MIN_SIDEBAR_WIDTH, Rect, TIMELINE_LARGE_SHARE, WindowLayout, rack_share_at,
    sidebar_width_at, toggled_timeline_height, window_layout, window_layout_with,
};
use fontelle_ui::theme::{Metrics, Theme};

const W: f32 = 1280.0;
const H: f32 = 720.0;

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn docks() -> Docks {
    Docks::default()
}

fn laid_out(docks: &Docks) -> WindowLayout {
    window_layout_with(W, H, &metrics(), docks)
}

// ---------------------------------------------------- the old way still ---

#[test]
fn the_defaults_are_what_the_window_had_before_any_of_this() {
    // `window_layout` is still the whole answer for a caller with no saved
    // sizes, and it must not have moved a pixel.
    let old = window_layout(W, H, &metrics(), DEFAULT_TIMELINE_HEIGHT);
    let new = laid_out(&docks());
    assert_eq!(old.rack.frame, new.rack.frame);
    assert_eq!(old.browser.frame, new.browser.frame);
    assert_eq!(old.panel.frame, new.panel.frame);
    assert_eq!(old.timeline.frame, new.timeline.frame);
}

// ------------------------------------------------------- the side seam ---

#[test]
fn the_seam_between_the_sidebar_and_the_editor_is_grabbable() {
    let l = laid_out(&docks());
    assert!(
        !l.sidebar_seam.is_empty(),
        "there has to be something to aim at"
    );
    assert!(
        l.sidebar_seam.x >= l.rack.frame.right(),
        "it is the gap, not a strip over the rack"
    );
    assert!(l.sidebar_seam.right() <= l.panel.frame.x);
    assert!(!l.sidebar_seam.intersects(&l.rack.frame));
    assert!(!l.sidebar_seam.intersects(&l.browser.frame));
    assert!(!l.sidebar_seam.intersects(&l.panel.frame));
    assert!(
        l.sidebar_seam.height >= l.rack.frame.height,
        "and it runs the height of the column, not just beside one panel"
    );
}

#[test]
fn dragging_the_seam_right_makes_the_sidebar_wider() {
    let l = laid_out(&docks());
    let wanted = sidebar_width_at(&l, 400.0);
    assert!(wanted > metrics().sidebar_width);

    let wider = laid_out(&Docks {
        sidebar_width: Some(wanted),
        ..docks()
    });
    assert!((wider.rack.frame.right() - 400.0).abs() < 1.0);
    assert!(wider.browser.frame.width > l.browser.frame.width);
    assert!(
        wider.panel.frame.width < l.panel.frame.width,
        "the room comes out of the editor, which is where it was"
    );
    assert_eq!(
        wider.panel.frame.right(),
        l.panel.frame.right(),
        "and the right-hand edge does not move"
    );
}

#[test]
fn the_sidebar_cannot_be_dragged_shut() {
    let l = laid_out(&docks());
    let narrow = sidebar_width_at(&l, 0.0);
    assert!(
        narrow >= MIN_SIDEBAR_WIDTH,
        "a column too narrow to read a soundfont's name in is not a column, got {narrow}"
    );
    let l = laid_out(&Docks {
        sidebar_width: Some(0.0),
        ..docks()
    });
    assert!(l.rack.frame.width >= MIN_SIDEBAR_WIDTH - 1.0);
}

#[test]
fn the_sidebar_cannot_eat_the_piano_roll() {
    let l = laid_out(&docks());
    let greedy = sidebar_width_at(&l, W);
    let l = laid_out(&Docks {
        sidebar_width: Some(greedy),
        ..docks()
    });
    assert!(
        l.panel.frame.width >= MIN_ROLL_WIDTH - 1.0,
        "the roll is what the window is for; it kept {}",
        l.panel.frame.width
    );
}

// ------------------------------------------------------ the split above ---

#[test]
fn the_seam_between_the_rack_and_the_browser_is_grabbable() {
    let l = laid_out(&docks());
    assert!(!l.sidebar_split.is_empty());
    assert!(l.sidebar_split.y >= l.rack.frame.bottom());
    assert!(l.sidebar_split.bottom() <= l.browser.frame.y);
    assert!(!l.sidebar_split.intersects(&l.rack.frame));
    assert!(!l.sidebar_split.intersects(&l.browser.frame));
    assert!(
        l.sidebar_split.width <= l.rack.frame.width + 1.0,
        "it belongs to the sidebar, not to the whole window"
    );
}

#[test]
fn dragging_the_split_up_gives_the_soundfonts_the_room() {
    // The report, in one test: the browser ends up taller than the rack.
    let l = laid_out(&docks());
    let share = rack_share_at(&l, l.rack.frame.y + 90.0);
    let l = laid_out(&Docks {
        rack_share: Some(share),
        ..docks()
    });
    assert!(
        l.browser.frame.height > l.rack.frame.height,
        "rack {} vs browser {}",
        l.rack.frame.height,
        l.browser.frame.height
    );
    assert!(l.rack.frame.height >= MIN_RACK_HEIGHT - 1.0);
}

#[test]
fn dragging_the_split_down_gives_the_channels_the_room() {
    let l = laid_out(&docks());
    let share = rack_share_at(&l, l.browser.frame.bottom() - 40.0);
    let l = laid_out(&Docks {
        rack_share: Some(share),
        ..docks()
    });
    assert!(l.rack.frame.height > l.browser.frame.height);
    assert!(
        l.browser.frame.height >= MIN_BROWSER_HEIGHT - 1.0,
        "a browser with no room for its search box is not a browser, got {}",
        l.browser.frame.height
    );
}

#[test]
fn neither_panel_can_be_dragged_away_entirely() {
    let l = laid_out(&docks());
    for y in [-2000.0_f32, -1.0, 0.0, H, 4000.0] {
        let share = rack_share_at(&l, y);
        assert!(
            (0.0..=1.0).contains(&share),
            "a share of {share} is not a share"
        );
        let l = laid_out(&Docks {
            rack_share: Some(share),
            ..docks()
        });
        assert!(l.rack.frame.height >= MIN_RACK_HEIGHT - 1.0);
        assert!(l.browser.frame.height >= MIN_BROWSER_HEIGHT - 1.0);
    }
}

// ------------------------------------------------------------ the edges ---

#[test]
fn the_seams_go_away_with_the_sidebar_they_separate() {
    // On a window too narrow for a sidebar there is no sidebar, and a seam
    // with nothing on one side of it is a strip that eats clicks.
    let l = window_layout_with(300.0, H, &metrics(), &docks());
    assert!(l.rack.frame.is_empty());
    assert!(l.sidebar_seam.is_empty());
    assert!(l.sidebar_split.is_empty());
}

#[test]
fn a_window_of_no_size_lays_out_without_a_negative_anything() {
    for (w, h) in [(0.0, 0.0), (1.0, 1.0), (40.0, 900.0), (900.0, 40.0)] {
        let l = window_layout_with(w, h, &metrics(), &docks());
        for rect in [
            l.rack.frame,
            l.browser.frame,
            l.panel.frame,
            l.sidebar_seam,
            l.sidebar_split,
        ] {
            assert!(
                rect.width >= 0.0 && rect.height >= 0.0,
                "{w}x{h} produced {rect:?}"
            );
        }
    }
}

#[test]
fn the_sidebar_seam_survives_the_arrangement_being_hidden() {
    let l = window_layout_with(
        W,
        H,
        &metrics(),
        &Docks {
            timeline_height: 0.0,
            ..docks()
        },
    );
    assert!(l.timeline.frame.is_empty());
    assert!(l.divider.is_empty(), "no arrangement, no arrangement seam");
    assert!(
        !l.sidebar_seam.is_empty(),
        "but the sidebar is still there and still draggable"
    );
    assert_eq!(l.sidebar_seam.height, l.panel.frame.height);
}

#[test]
fn every_seam_is_a_gap_and_not_an_overlap() {
    let l = laid_out(&Docks {
        sidebar_width: Some(300.0),
        rack_share: Some(0.3),
        ..docks()
    });
    let rects = [
        ("rack", l.rack.frame),
        ("browser", l.browser.frame),
        ("timeline", l.timeline.frame),
        ("panel", l.panel.frame),
    ];
    for (name, rect) in rects {
        for (seam_name, seam) in [
            ("sidebar_seam", l.sidebar_seam),
            ("sidebar_split", l.sidebar_split),
            ("divider", l.divider),
        ] {
            assert!(
                !rect.intersects(&seam),
                "{seam_name} {seam:?} overlaps {name} {rect:?}"
            );
        }
    }
    assert!(!l.sidebar_seam.intersects(&l.sidebar_split));
}

#[test]
fn a_seam_is_wide_enough_to_hit_without_aiming() {
    let l = laid_out(&docks());
    assert!(l.sidebar_seam.width >= 4.0, "{:?}", l.sidebar_seam);
    assert!(l.sidebar_split.height >= 4.0, "{:?}", l.sidebar_split);
    assert!(
        Rect::new(0.0, 0.0, 0.0, 0.0).is_empty(),
        "sanity: an empty rect is empty"
    );
}

// ------------------------------------------------ Tab swaps the two heights ---

/// The height of everything the arrangement and the editor share: the two
/// frames and the seam between them.
fn column_height(layout: &WindowLayout) -> f32 {
    layout
        .timeline
        .frame
        .union(&layout.divider)
        .union(&layout.panel.frame)
        .height
}

#[test]
fn tab_makes_the_arrangement_the_big_one_when_the_editor_is_and_the_other_way_round() {
    // *"pressing tab should toggle between the piano/mixer section being
    // larger or smaller than the arrangement section vertically ... about a
    // 66.66%/33.33% ratio."* Two positions, and the press goes to whichever
    // the window is not in.
    assert!((TIMELINE_LARGE_SHARE - 2.0 / 3.0).abs() < 1e-6);
    // A window tall enough that neither position is clamped.
    let tall = window_layout_with(W, 1200.0, &metrics(), &docks());
    let column = column_height(&tall);
    let big = toggled_timeline_height(&tall);
    assert!(
        (big / column - TIMELINE_LARGE_SHARE).abs() < 0.02,
        "from the default (editor bigger) Tab gives the arrangement two thirds; got {}",
        big / column
    );
    let now_big = window_layout_with(
        W,
        1200.0,
        &metrics(),
        &Docks {
            timeline_height: big,
            ..docks()
        },
    );
    let small = toggled_timeline_height(&now_big);
    assert!(
        (small / column - (1.0 - TIMELINE_LARGE_SHARE)).abs() < 0.02,
        "from two thirds Tab gives the arrangement one third; got {}",
        small / column
    );
    let now_small = window_layout_with(
        W,
        1200.0,
        &metrics(),
        &Docks {
            timeline_height: small,
            ..docks()
        },
    );
    assert!(
        (toggled_timeline_height(&now_small) - big).abs() < 0.5,
        "and back again"
    );
}

#[test]
fn after_a_free_drag_tab_goes_to_whichever_position_swaps_who_is_bigger() {
    // *"pressing it after freely adjusting just goes back to toggling through
    // those positioning modes."* No memory of a mode: the rule is simply
    // "if the arrangement has more than half, give it a third; otherwise
    // give it two thirds", which is the same answer at both fixed positions
    // and a sensible one from anywhere in between.
    let column = column_height(&window_layout_with(W, 1200.0, &metrics(), &docks()));
    for share in [0.05, 0.3, 0.49, 0.51, 0.6, 0.9] {
        let laid = window_layout_with(
            W,
            1200.0,
            &metrics(),
            &Docks {
                timeline_height: column * share,
                ..docks()
            },
        );
        let next = toggled_timeline_height(&laid) / column;
        let wanted = if share >= 0.5 {
            1.0 - TIMELINE_LARGE_SHARE
        } else {
            TIMELINE_LARGE_SHARE
        };
        assert!(
            (next - wanted).abs() < 0.02,
            "from {share} Tab went to {next}, wanted {wanted}"
        );
    }
}

#[test]
fn tab_brings_a_hidden_arrangement_back_as_the_big_one() {
    let hidden = window_layout_with(
        W,
        1200.0,
        &metrics(),
        &Docks {
            timeline_height: 0.0,
            ..docks()
        },
    );
    assert!(hidden.timeline.frame.is_empty());
    let column = column_height(&hidden);
    let shown = toggled_timeline_height(&hidden);
    assert!(
        shown > column * 0.5,
        "an arrangement that was hidden comes back as the larger panel, got {shown} of {column}"
    );
}

#[test]
fn tab_never_squeezes_the_editor_under_its_minimum() {
    // A short window cannot give the arrangement two thirds without taking
    // the editor under `MIN_EDITOR_HEIGHT`, and the same clamp the seam drag
    // has applies here.
    let short = window_layout_with(W, 420.0, &metrics(), &docks());
    let height = toggled_timeline_height(&short);
    let laid = window_layout_with(
        W,
        420.0,
        &metrics(),
        &Docks {
            timeline_height: height,
            ..docks()
        },
    );
    assert!(
        laid.panel.frame.height >= MIN_EDITOR_HEIGHT - 0.01,
        "the editor was squeezed to {}",
        laid.panel.frame.height
    );
    assert!(height >= 0.0);
}
