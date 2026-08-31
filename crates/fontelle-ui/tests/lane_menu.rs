//! Choosing what the property lane shows, from a list you can see.
//!
//! Reported from using the window: *"I'm still not seeing panning options, only
//! the velocity in the piano roll."*
//!
//! Pan was there. Every property `Note` carries was there — the lane has drawn
//! any of them since it was written, and the toolbar chip cycled through them.
//! The chip said `vel`, which is a perfectly good *read-out* of what the lane
//! is showing and a completely invisible *control*: nothing about a small box
//! containing the word "vel" says "the other five are behind this". So the
//! feature existed and did not exist, which is the worse of the two.
//!
//! A menu is the fix, and it is the fix precisely because it lists what it can
//! do without being asked. `L` still cycles for anyone who has learnt it.

use fontelle_ui::canvas::{
    LANE_PROPERTIES, LaneProperty, RollControl, lane_menu_hit, lane_menu_layout, roll_layout,
    toolbar_layout,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn panel() -> Rect {
    Rect::new(300.0, 40.0, 900.0, 600.0)
}

/// Where the lane chip actually is, so these tests are about the real chip
/// rather than a rectangle invented here.
fn chip(frame: Rect) -> Rect {
    let m = metrics();
    let layout = roll_layout(frame, &m, 78.0);
    let bar = toolbar_layout(layout.toolbar, &m);
    bar.items
        .iter()
        .find(|(control, _)| *control == RollControl::Lane)
        .map(|(_, rect)| *rect)
        .expect("the toolbar has a lane chip")
}

#[test]
fn every_property_the_lane_can_show_is_on_the_menu() {
    let frame = panel();
    let menu = lane_menu_layout(chip(frame), frame, &metrics());
    let listed: Vec<LaneProperty> = menu.items.iter().map(|(p, _)| *p).collect();
    assert_eq!(listed, LANE_PROPERTIES.to_vec());
    assert!(
        listed.contains(&LaneProperty::Pan),
        "which is the one that was reported missing"
    );
}

#[test]
fn the_items_stack_without_gaps_or_overlaps_and_fill_the_frame() {
    let frame = panel();
    let menu = lane_menu_layout(chip(frame), frame, &metrics());
    let mut previous: Option<Rect> = None;
    for (property, rect) in &menu.items {
        assert!(!rect.is_empty(), "{property:?} has nowhere to draw");
        assert!(
            menu.frame.intersection(rect).height >= rect.height - 0.01,
            "{property:?} escaped the menu"
        );
        if let Some(above) = previous {
            assert!(
                (rect.y - above.bottom()).abs() < 0.01,
                "{property:?} does not sit on the one above it"
            );
        }
        previous = Some(*rect);
    }
}

#[test]
fn it_drops_from_the_chip_and_lines_up_with_it() {
    let frame = panel();
    let chip = chip(frame);
    let menu = lane_menu_layout(chip, frame, &metrics());
    assert!(
        menu.frame.y >= chip.bottom() - 0.01,
        "a menu that covers the control it belongs to hides what it is doing"
    );
    assert!(
        (menu.frame.x - chip.x).abs() < 24.0,
        "it hangs off the chip"
    );
    assert!(
        menu.frame.width >= chip.width,
        "and is at least as wide as the thing it came from"
    );
}

#[test]
fn it_stays_inside_the_panel_it_belongs_to() {
    // A menu drawn past the panel's edge is a menu drawn over the sidebar, or
    // over nothing at all.
    for frame in [
        panel(),
        Rect::new(300.0, 40.0, 200.0, 600.0),
        Rect::new(0.0, 0.0, 900.0, 240.0),
    ] {
        let menu = lane_menu_layout(chip(frame), frame, &metrics());
        let inside = frame.intersection(&menu.frame);
        assert!(
            (inside.width - menu.frame.width).abs() < 0.01
                && (inside.height - menu.frame.height).abs() < 0.01,
            "{:?} is not inside {frame:?}",
            menu.frame
        );
    }
}

#[test]
fn it_opens_upwards_when_there_is_no_room_below() {
    // A short panel — the roll docked small under a tall arrangement.
    let frame = Rect::new(300.0, 40.0, 900.0, 150.0);
    let chip = chip(frame);
    let menu = lane_menu_layout(chip, frame, &metrics());
    assert!(
        menu.frame.bottom() <= frame.bottom() + 0.01,
        "it must not hang out of the bottom of the panel"
    );
    assert!(!menu.items.is_empty(), "and it must still list something");
}

#[test]
fn clicking_a_row_names_the_property_it_is_drawn_with() {
    let frame = panel();
    let menu = lane_menu_layout(chip(frame), frame, &metrics());
    for (property, rect) in &menu.items {
        let hit = lane_menu_hit(&menu, rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        assert_eq!(hit, Some(*property));
    }
}

#[test]
fn clicking_anywhere_else_names_nothing() {
    let frame = panel();
    let menu = lane_menu_layout(chip(frame), frame, &metrics());
    for (x, y) in [
        (frame.x + 1.0, frame.bottom() - 1.0),
        (menu.frame.x - 4.0, menu.frame.y + 4.0),
        (menu.frame.right() + 4.0, menu.frame.y + 4.0),
        (menu.frame.x + 4.0, menu.frame.bottom() + 4.0),
    ] {
        assert_eq!(
            lane_menu_hit(&menu, x, y),
            None,
            "({x}, {y}) is outside the menu"
        );
    }
}

#[test]
fn a_menu_with_nowhere_to_go_lists_nothing_rather_than_drawing_a_sliver() {
    let frame = Rect::new(0.0, 0.0, 0.0, 0.0);
    let menu = lane_menu_layout(Rect::ZERO, frame, &metrics());
    assert!(menu.items.is_empty());
    assert!(menu.frame.is_empty());
    assert_eq!(lane_menu_hit(&menu, 0.0, 0.0), None);
}

#[test]
fn the_cycle_and_the_menu_agree_on_what_exists() {
    // Two lists of the same six things is one list to forget to update.
    let mut seen = Vec::new();
    let mut property = LaneProperty::Velocity;
    for _ in 0..LANE_PROPERTIES.len() {
        seen.push(property);
        property = property.next();
    }
    assert_eq!(property, LaneProperty::Velocity, "the cycle closes");
    let mut sorted_seen: Vec<&str> = seen.iter().map(|p| p.label()).collect();
    let mut sorted_menu: Vec<&str> = LANE_PROPERTIES.iter().map(|p| p.label()).collect();
    sorted_seen.sort_unstable();
    sorted_menu.sort_unstable();
    assert_eq!(sorted_seen, sorted_menu);
}
