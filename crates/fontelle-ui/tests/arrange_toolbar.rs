//! The arrangement's toolbar: the controls it never had.
//!
//! Reported from using the window: *"we need better arrangement controls like
//! looping, duplicating clips, copying and pasting arrangement clips, cutting,
//! etc. Right now I can only change the length and move them around"* — and,
//! separately, *"I also don't see snap controls right now, please ensure we
//! have those."*
//!
//! Both are the same gap. `TimelineView` has carried a `snap` since it was
//! written and `Timeline::duplicate` has existed since the arrangement did;
//! neither had anywhere to appear, because the panel was a ruler, a header
//! column and a grid. `Ctrl+B` worked if you knew it existed and had clicked
//! the arrangement last, which is not a control, it is a rumour.

use fontelle_ui::canvas::{
    SnapDivision, Timeline, TimelineControl, TimelineView, timeline_layout, timeline_toolbar_hit,
    timeline_toolbar_layout,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn m() -> Metrics {
    Theme::dark_default().metrics
}

fn frame() -> Rect {
    Rect::new(0.0, 0.0, 1000.0, 260.0)
}

fn bar() -> fontelle_ui::canvas::TimelineToolbar {
    timeline_toolbar_layout(timeline_layout(frame(), &m()).toolbar, &m())
}

#[test]
fn the_toolbar_carries_the_controls_the_arrangement_was_missing() {
    let items: Vec<TimelineControl> = bar().items.iter().map(|(c, _)| *c).collect();

    for wanted in [
        TimelineControl::Snap,
        TimelineControl::Repeat,
        TimelineControl::Cut,
        TimelineControl::Copy,
        TimelineControl::Paste,
        TimelineControl::Mute,
    ] {
        assert!(
            items.contains(&wanted),
            "{wanted:?} is one of the things the report asked for"
        );
    }
}

#[test]
fn every_control_answers_a_click_in_the_middle_of_itself() {
    let bar = bar();
    for (control, rect) in &bar.items {
        let hit = timeline_toolbar_hit(&bar, rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        assert_eq!(hit, Some(*control), "{control:?} at {rect:?}");
    }
}

#[test]
fn the_controls_do_not_overlap_each_other() {
    let bar = bar();
    for (i, (a, ra)) in bar.items.iter().enumerate() {
        for (b, rb) in bar.items.iter().skip(i + 1) {
            assert!(
                !ra.intersects(rb),
                "{a:?} and {b:?} are on top of each other"
            );
        }
    }
}

#[test]
fn a_click_off_the_end_of_the_toolbar_hits_nothing() {
    let bar = bar();
    let l = timeline_layout(frame(), &m());
    assert_eq!(
        timeline_toolbar_hit(&bar, l.toolbar.right() - 1.0, l.toolbar.y + 2.0),
        None
    );
    assert_eq!(timeline_toolbar_hit(&bar, -5.0, l.toolbar.y + 2.0), None);
}

/// A panel too narrow for every control drops the ones that do not fit rather
/// than drawing them on top of one another — the same rule the roll's toolbar
/// follows.
#[test]
fn a_narrow_panel_drops_controls_instead_of_stacking_them() {
    for width in [0.0, 30.0, 90.0, 200.0] {
        let l = timeline_layout(Rect::new(0.0, 0.0, width, 260.0), &m());
        let bar = timeline_toolbar_layout(l.toolbar, &m());
        for (_, rect) in &bar.items {
            assert!(rect.width >= 0.0 && rect.height >= 0.0, "{width}: {rect:?}");
            assert!(
                rect.right() <= l.toolbar.right() + 0.01,
                "{width}: a control hangs off the end at {rect:?}"
            );
        }
    }
}

/// The snap chip cycles, and it is the arrangement's own division — the roll
/// and the arrangement are two grids and a person sets them separately.
#[test]
fn the_snap_chip_cycles_the_arrangements_own_division() {
    let mut timeline = Timeline::new(TimelineView {
        snap: SnapDivision::Bar,
        ..TimelineView::default()
    });
    timeline.cycle_snap();
    assert_eq!(timeline.view.snap, SnapDivision::Bar.next());

    // And all the way round, so the chip can never strand itself.
    let mut seen = 0;
    let first = timeline.view.snap;
    loop {
        timeline.cycle_snap();
        seen += 1;
        if timeline.view.snap == first || seen > 32 {
            break;
        }
    }
    assert!(seen <= 32, "the snap cycle does not come back round");
}
