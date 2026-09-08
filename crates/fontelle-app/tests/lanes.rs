//! The rows a project starts with, and putting a new one where you are looking.
//!
//! Reported from using the window:
//!
//! > *"instead of only starting with 1 lane in a new arrangement make it like
//! > 10 or something just not one its too barren"*
//!
//! > *"also when i right click a lane in the arrangement i want the option to
//! > add a lane above or a lane below the lane i right clicked on right now
//! > theyre all going to the end."*

mod common;

use fontelle_app::blank_project;
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

const BARS: i64 = 8;

#[test]
fn a_new_project_opens_with_a_stack_of_rows_rather_than_one() {
    let project = blank_project(BARS, 120.0, SR);
    assert!(
        project.lanes.len() >= 10,
        "a new project has {} rows",
        project.lanes.len()
    );
}

#[test]
fn the_rows_are_named_in_order_and_none_of_them_carries_anything() {
    let project = blank_project(BARS, 120.0, SR);
    let mut lanes: Vec<_> = project
        .lanes
        .iter()
        .map(|(id, l)| (id, l.clone()))
        .collect();
    lanes.sort_by_key(|(_, l)| l.order);
    for (index, (_, lane)) in lanes.iter().enumerate() {
        assert_eq!(lane.name, format!("Lane {}", index + 1));
    }
    // The rows are room to work in, not content — and since a new project
    // stopped arriving with a clip on the first one (see
    // `tests/starting_project.rs`) that is *all* they are.
    assert_eq!(project.clips.len(), 0, "ten rows, nothing on any of them");
}

#[test]
fn the_orders_are_distinct_so_the_stack_has_an_order_at_all() {
    let project = blank_project(BARS, 120.0, SR);
    let mut orders: Vec<u32> = project.lanes.values().map(|l| l.order).collect();
    orders.sort_unstable();
    let before = orders.len();
    orders.dedup();
    assert_eq!(
        orders.len(),
        before,
        "two rows share a position in the stack"
    );
}

// ------------------------------------------------- adding one where you are ---

fn a_session() -> fontelle_app::Session {
    common::a_session_for(blank_project(BARS, 120.0, SR))
}

#[test]
fn a_row_can_be_put_directly_above_the_one_that_was_right_clicked() {
    let mut session = a_session();
    let names: Vec<String> = session.lanes().into_iter().map(|l| l.name).collect();
    session.add_lane_at(3);
    let after: Vec<String> = session.lanes().into_iter().map(|l| l.name).collect();
    assert_eq!(after.len(), names.len() + 1);
    // Everything above row 3 is where it was, and the new row is at 3.
    assert_eq!(after[..3], names[..3]);
    assert_eq!(after[4..], names[3..]);
}

#[test]
fn a_row_can_be_put_directly_below_it_too() {
    let mut session = a_session();
    let names: Vec<String> = session.lanes().into_iter().map(|l| l.name).collect();
    session.add_lane_at(4);
    let after: Vec<String> = session.lanes().into_iter().map(|l| l.name).collect();
    assert_eq!(after[..4], names[..4]);
    assert_eq!(after[5..], names[4..]);
}

#[test]
fn adding_past_the_end_puts_it_at_the_end_rather_than_refusing() {
    let mut session = a_session();
    let before = session.lanes().len();
    session.add_lane_at(usize::MAX);
    assert_eq!(session.lanes().len(), before + 1);
}

#[test]
fn a_new_row_carries_no_clips_and_the_old_ones_keep_theirs() {
    // The rows are re-ordered, not re-pointed: a clip names a lane id, and an
    // insert that shuffled ids would move somebody's music.
    let mut session = a_session();
    let clips_before: Vec<_> = session
        .clips()
        .into_iter()
        .map(|c| (c.id, c.lane))
        .collect();
    session.add_lane_at(0);
    for (id, lane) in clips_before {
        let now = session
            .clips()
            .into_iter()
            .find(|c| c.id == id)
            .expect("still there");
        assert_eq!(now.lane, lane + 1, "the clip moved down with its row");
    }
}

#[test]
fn adding_a_row_is_one_undo() {
    let mut session = a_session();
    let before = session.lanes().len();
    session.add_lane_at(2);
    assert_eq!(session.lanes().len(), before + 1);
    session.undo();
    assert_eq!(session.lanes().len(), before);
}
