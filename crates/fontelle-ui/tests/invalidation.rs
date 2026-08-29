//! TDD §16.3's hard requirement, made checkable: **when nothing is animating,
//! issue no frames at all.**
//!
//! This is the idle-CPU target (§19: under 0.5% of one core with the transport
//! stopped) and the plan is explicit that it is much easier to build in than to
//! retrofit. So it is a pure decision — "is there a frame to draw, and over
//! what rectangle" — and it is decided here, not inside an event loop where
//! nothing could test it.

use fontelle_ui::layout::Rect;
use fontelle_ui::widget::{Redraw, WidgetId, WidgetTree};

#[test]
fn an_idle_surface_issues_no_frames_at_all() {
    let mut redraw = Redraw::new();
    assert!(!redraw.is_dirty());
    for _ in 0..1000 {
        assert_eq!(
            redraw.take_dirty(),
            None,
            "a clean surface asked for a frame"
        );
    }
}

#[test]
fn one_invalidation_buys_exactly_one_frame() {
    let mut redraw = Redraw::new();
    let rect = Rect::new(10.0, 10.0, 100.0, 20.0);
    redraw.invalidate(rect);

    assert!(redraw.is_dirty());
    assert_eq!(redraw.take_dirty(), Some(rect));
    // And then it is quiet again. A frame that redraws itself forever is the
    // exact failure §16.3 exists to prevent.
    assert!(!redraw.is_dirty());
    assert_eq!(redraw.take_dirty(), None);
}

#[test]
fn invalidations_between_frames_coalesce_into_one_region() {
    let mut redraw = Redraw::new();
    redraw.invalidate(Rect::new(0.0, 0.0, 10.0, 10.0));
    redraw.invalidate(Rect::new(90.0, 40.0, 10.0, 10.0));

    // Three widgets changing between two vsyncs is one frame, not three.
    assert_eq!(redraw.take_dirty(), Some(Rect::new(0.0, 0.0, 100.0, 50.0)));
    assert_eq!(redraw.take_dirty(), None);
}

#[test]
fn invalidating_nothing_is_not_a_frame() {
    let mut redraw = Redraw::new();
    redraw.invalidate(Rect::ZERO);
    redraw.invalidate(Rect::new(5.0, 5.0, 0.0, 30.0));
    assert!(
        !redraw.is_dirty(),
        "an empty rectangle asked for a frame it has nothing to draw in"
    );
}

#[test]
fn an_animator_asks_the_event_loop_to_keep_running_and_stops_asking() {
    let mut redraw = Redraw::new();
    assert!(!redraw.is_animating());

    // The playhead is the first of these (item 7): while it moves, the loop
    // must not go to sleep waiting for input.
    redraw.begin_animating();
    assert!(redraw.is_animating());
    redraw.end_animating();
    assert!(!redraw.is_animating());
}

#[test]
fn animators_nest_so_two_moving_things_do_not_cancel_each_other() {
    let mut redraw = Redraw::new();
    redraw.begin_animating();
    redraw.begin_animating();
    redraw.end_animating();
    assert!(
        redraw.is_animating(),
        "one of two animators stopping put the window to sleep"
    );
    redraw.end_animating();
    assert!(!redraw.is_animating());
    // Over-releasing is a bug in the caller, not a reason to underflow.
    redraw.end_animating();
    assert!(!redraw.is_animating());
}

#[test]
fn animating_is_not_the_same_as_dirty() {
    let mut redraw = Redraw::new();
    redraw.begin_animating();
    // An animator keeps the loop awake; it does not itself claim pixels. What
    // moves has to say where, which is what keeps "the playhead moving must
    // not redirty the note geometry" (§16.4) true by construction.
    assert!(!redraw.is_dirty());
    assert_eq!(redraw.take_dirty(), None);
}

#[test]
fn a_widget_invalidates_only_its_own_bounds() {
    let mut tree = WidgetTree::new();
    let playhead = WidgetId::new(1);
    let notes = WidgetId::new(2);
    tree.insert(playhead, Rect::new(400.0, 0.0, 2.0, 600.0));
    tree.insert(notes, Rect::new(0.0, 0.0, 1280.0, 600.0));
    tree.take_dirty();

    tree.invalidate(playhead);
    assert_eq!(
        tree.take_dirty(),
        Some(Rect::new(400.0, 0.0, 2.0, 600.0)),
        "the playhead redirtied more than the playhead"
    );
}

#[test]
fn moving_a_widget_dirties_where_it_was_and_where_it_went() {
    let mut tree = WidgetTree::new();
    let playhead = WidgetId::new(1);
    tree.insert(playhead, Rect::new(400.0, 0.0, 2.0, 600.0));
    tree.take_dirty();

    tree.set_bounds(playhead, Rect::new(408.0, 0.0, 2.0, 600.0));
    // Only redrawing the new position leaves the old one smeared across the
    // screen — the classic dirty-region bug.
    assert_eq!(tree.take_dirty(), Some(Rect::new(400.0, 0.0, 10.0, 600.0)));
    assert_eq!(
        tree.bounds(playhead),
        Some(Rect::new(408.0, 0.0, 2.0, 600.0))
    );
}

#[test]
fn a_new_widget_is_dirty_and_an_unknown_one_is_not_an_error() {
    let mut tree = WidgetTree::new();
    let id = WidgetId::new(7);
    tree.insert(id, Rect::new(1.0, 2.0, 3.0, 4.0));
    assert!(
        tree.has_dirty_regions(),
        "a widget appeared without asking to be drawn"
    );
    tree.take_dirty();

    tree.invalidate(WidgetId::new(99));
    assert!(!tree.has_dirty_regions());
    assert_eq!(tree.bounds(WidgetId::new(99)), None);
}

#[test]
fn a_resize_dirties_the_whole_surface() {
    let mut tree = WidgetTree::new();
    tree.insert(WidgetId::new(1), Rect::new(0.0, 0.0, 10.0, 10.0));
    tree.take_dirty();

    tree.invalidate_rect(Rect::new(0.0, 0.0, 1920.0, 1080.0));
    assert_eq!(tree.take_dirty(), Some(Rect::new(0.0, 0.0, 1920.0, 1080.0)));
}

// --------------------------------------------- how long may we sleep? ------

use fontelle_ui::widget::{ENGINE_POLL, FRAME_INTERVAL, Sleep, sleep_budget};

#[test]
fn a_window_with_nothing_behind_it_sleeps_until_the_os_speaks() {
    // No engine, nothing animating: there is no way for anything on this side
    // to change, so waking up at all would be waking up for nothing. This is
    // the §19 idle case and it is the common one.
    assert_eq!(sleep_budget(false, false), Sleep::Forever);
}

#[test]
fn a_window_watching_an_engine_looks_at_it_even_while_idle() {
    // The bug this exists to prevent, found by running it: the transport is
    // shared state that something *other than the window* can change — a
    // MIDI-triggered record, a second view, the CLI that opened it. A window
    // asleep in `Wait` never learns that playback started, and what the user
    // sees is a frozen playhead and a dead meter over audio they can hear.
    //
    // Note what this is not: it is not a frame. The loop wakes, reads six
    // atomics, finds nothing changed, marks nothing dirty and goes back to
    // sleep without drawing. Zero frames at idle (§16.3) is about frames.
    assert_eq!(sleep_budget(false, true), Sleep::AtMost(ENGINE_POLL));
}

#[test]
fn something_moving_is_drawn_at_the_frame_rate_whatever_else_is_true() {
    for watching in [false, true] {
        assert_eq!(
            sleep_budget(true, watching),
            Sleep::AtMost(FRAME_INTERVAL),
            "an animating window did not ask for the next frame"
        );
    }
}

#[test]
fn polling_the_engine_is_far_slower_than_drawing() {
    // If these ever cross, an idle window would be waking more often than a
    // busy one.
    assert!(ENGINE_POLL > FRAME_INTERVAL * 4);
}
