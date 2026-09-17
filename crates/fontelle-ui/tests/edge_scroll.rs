//! Edge scrolling, in time rather than in mouse events.
//!
//! Reported from using the window:
//!
//! > *"when i drag things they often go wayyyy off into infinity for me like
//! > with the slightest mouse movement for me pretty much universally all
//! > places i can drag something that moves the view"*
//!
//! The old `edge_scroll` answered *how far should the view move because of
//! this pointer event*, and the window applied it once per `CursorMoved`. So
//! the speed of the scroll was the **mouse's report rate**, which nobody chose
//! and which differs by an order of magnitude between one mouse and the next:
//! a gaming mouse at 1000 Hz scrolled a hundred times faster than a 100 Hz one
//! for the same gesture. Its own test bounded the travel *per event*, which is
//! the wrong invariant — a thousand bounded events a second is still a
//! thousand of them.
//!
//! So the rate is now **per second**, and the window integrates it against a
//! real clock. Two things fall out, and both are tests here: a pointer held
//! still travels the same distance however many events arrive while it is
//! held, and a rate too slow to move a whole tick in one step is remembered
//! rather than truncated to nothing.

use fontelle_types::Tick;
use fontelle_ui::canvas::{EdgeScroll, RollView, SnapDivision, edge_scroll_rate};
use fontelle_ui::layout::Rect;

fn view() -> RollView {
    RollView {
        scroll_tick: 0,
        top_key: 84,
        key_offset: 0.0,
        pixels_per_tick: 0.05,
        key_height: 12.0,
        snap: SnapDivision::Step,
    }
}

fn grid() -> Rect {
    Rect::new(100.0, 50.0, 800.0, 400.0)
}

// ------------------------------------------------------------- direction ---

#[test]
fn a_pointer_inside_the_grid_scrolls_nothing() {
    let (ticks, rows) = edge_scroll_rate(&view(), grid(), grid().x + 10.0, grid().y + 10.0);
    assert_eq!((ticks, rows), (0.0, 0.0));
}

#[test]
fn each_edge_scrolls_towards_itself() {
    let (v, g) = (view(), grid());
    assert!(
        edge_scroll_rate(&v, g, g.x - 40.0, g.y + 10.0).0 < 0.0,
        "left goes back"
    );
    assert!(
        edge_scroll_rate(&v, g, g.right() + 40.0, g.y + 10.0).0 > 0.0,
        "right goes on"
    );
    assert!(
        edge_scroll_rate(&v, g, g.x + 10.0, g.y - 40.0).1 > 0.0,
        "up the keyboard"
    );
    assert!(
        edge_scroll_rate(&v, g, g.x + 10.0, g.bottom() + 40.0).1 < 0.0,
        "down it"
    );
}

#[test]
fn further_out_is_faster_but_it_is_bounded() {
    let (v, g) = (view(), grid());
    let near = edge_scroll_rate(&v, g, g.x - 10.0, g.y + 10.0).0.abs();
    let far = edge_scroll_rate(&v, g, g.x - 300.0, g.y + 10.0).0.abs();
    let absurd = edge_scroll_rate(&v, g, g.x - 100_000.0, g.y + 10.0).0.abs();
    assert!(far > near, "a shove moves more than a nudge");
    assert!(
        (absurd - far).abs() < far,
        "and a pointer thrown off the desk is not a different feature: {absurd} vs {far}"
    );
    // A second at full pelt is a few screens, not a thousand bars.
    let screens = absurd * v.pixels_per_tick / g.width;
    assert!(
        (1.0..=6.0).contains(&screens),
        "a second at the fastest is {screens} screenfuls"
    );
}

// ------------------------------------------------------- and in the window ---

#[test]
fn the_same_gesture_travels_the_same_however_many_events_it_arrives_in() {
    // The whole of the report. One second of a pointer held 50 px outside the
    // grid: once as ten lazy events, once as a thousand from a gaming mouse.
    // They have to agree, or the mouse is deciding how far the drag goes.
    let (v, g) = (view(), grid());
    let rate = edge_scroll_rate(&v, g, g.x - 50.0, g.y + 10.0);

    let travel = |steps: u32| {
        let mut edge = EdgeScroll::default();
        let dt = 1.0 / steps as f32;
        let mut total: Tick = 0;
        for _ in 0..steps {
            total += edge.step(rate, dt).0;
        }
        total
    };

    let lazy = travel(10);
    let gaming = travel(1000);
    assert!(lazy < 0, "it did move");
    assert!(
        (lazy - gaming).abs() <= 2,
        "ten events said {lazy} and a thousand said {gaming}"
    );
}

#[test]
fn a_step_too_small_to_move_a_tick_is_remembered_rather_than_thrown_away() {
    // The trap in going per-second: at a fine zoom one millisecond of travel
    // is a fraction of a tick, and truncating each step to a whole tick would
    // scroll *nothing at all* on a fast mouse — the opposite failure, and a
    // harder one to see.
    let (v, g) = (view(), grid());
    let rate = edge_scroll_rate(&v, g, g.x - 20.0, g.y + 10.0);
    let mut edge = EdgeScroll::default();
    let mut total: Tick = 0;
    for _ in 0..1000 {
        total += edge.step(rate, 0.001).0;
    }
    assert!(
        total < 0,
        "a thousand small steps still went somewhere: {total}"
    );
}

#[test]
fn a_rate_of_nothing_moves_nothing_however_long_it_is_held() {
    let mut edge = EdgeScroll::default();
    for _ in 0..100 {
        assert_eq!(edge.step((0.0, 0.0), 0.016), (0, 0));
    }
}

#[test]
fn what_has_not_added_up_yet_is_dropped_when_the_gesture_ends() {
    // A fraction of a tick left over from one drag must not turn up at the
    // front of the next one, which is a clip that jumps as you take hold of it.
    let (v, g) = (view(), grid());
    let rate = edge_scroll_rate(&v, g, g.x - 20.0, g.y + 10.0);
    let mut edge = EdgeScroll::default();
    edge.step(rate, 0.001);
    edge.reset();
    assert_eq!(edge, EdgeScroll::default());
}

#[test]
fn a_wild_time_step_cannot_throw_the_view() {
    // A frame the compositor sat on, a laptop resumed from sleep, a debugger
    // breakpoint: `dt` is wall-clock and can be anything. The scroll is a
    // gesture, not a physics simulation, so a huge gap is one step's worth.
    let (v, g) = (view(), grid());
    let rate = edge_scroll_rate(&v, g, g.x - 50.0, g.y + 10.0);
    let mut edge = EdgeScroll::default();
    let (huge, _) = edge.step(rate, 45.0);
    let mut edge = EdgeScroll::default();
    let (second, _) = edge.step(rate, 1.0);
    assert!(
        huge.abs() <= second.abs(),
        "45 seconds of catch-up moved {huge} against one second's {second}"
    );
}
