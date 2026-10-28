//! G steps the snap of the panel being worked on: *"right now when pressing
//! g in the piano roll it changes your grid snapping but in the arrangement
//! it doesnt do that when you press g can you make sure it lets you do that
//! in both ?"* (Ty, 2026-10-27).

mod common;

use fontelle_ui::canvas::Action;

fn a_window() -> fontelle_ui::Windowless {
    fontelle_ui::Windowless::new(
        Box::new(common::a_session_for(common::a_clip_project(8))),
        fontelle_ui::theme::Theme::dark_default(),
        (1600, 1000),
    )
}

#[test]
fn g_on_the_arrangement_steps_the_arrangements_snap_and_not_the_rolls() {
    let mut w = a_window();
    let (ruler, _, before) = w.timeline_ruler_and_view();
    let (_, _, roll_before) = w.roll_ruler_and_view();
    w.click_studio(ruler.x + 40.0, ruler.y + ruler.height / 2.0);
    w.act(Action::RollSnap);
    let (_, _, after) = w.timeline_ruler_and_view();
    let (_, _, roll_after) = w.roll_ruler_and_view();
    assert_eq!(
        after.snap,
        before.snap.next(),
        "the arrangement's snap did not step"
    );
    assert_eq!(
        roll_after.snap, roll_before.snap,
        "the roll's snap moved instead"
    );
}

#[test]
fn g_in_the_roll_still_steps_the_rolls_snap() {
    let mut w = a_window();
    let (ruler, _, roll_before) = w.roll_ruler_and_view();
    let (_, _, before) = w.timeline_ruler_and_view();
    w.click_studio(ruler.x + 40.0, ruler.y + ruler.height / 2.0);
    w.act(Action::RollSnap);
    let (_, _, roll_after) = w.roll_ruler_and_view();
    let (_, _, after) = w.timeline_ruler_and_view();
    assert_eq!(roll_after.snap, roll_before.snap.next());
    assert_eq!(after.snap, before.snap);
}
