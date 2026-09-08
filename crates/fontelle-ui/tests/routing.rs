//! The channel rack's route chip, and the menu it opens (TDD §13.1).
//!
//! Reported from using the window:
//!
//! > *"From the channels tab, each channel should have a dropdown where you
//! > can choose from any of the available mixer tracks."*
//!
//! The document half of this is `fontelle-model`'s `tests/routing.rs`: a
//! channel no longer owns a mixer track, it *points* at one, and `None` is the
//! master. This file is the half that makes the pointing visible and
//! changeable — a chip on every row saying where the channel goes, and a menu
//! listing everywhere it could go instead.
//!
//! Geometry and hit-testing only, and both pure, per §2.5 of the plan.

use fontelle_ui::canvas::{
    RackHit, RouteChoice, output_menu_layout, rack_hit, rack_layout, route_menu_hit,
    route_menu_layout,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn body() -> Rect {
    Rect::new(8.0, 40.0, 248.0, 300.0)
}

// ------------------------------------------------------------ the chip ---

#[test]
fn every_row_carries_a_route_chip_and_it_does_not_eat_the_other_controls() {
    let m = metrics();
    let l = rack_layout(body(), &m, 4, 0);

    for row in &l.rows {
        assert!(!row.route.is_empty(), "row {} has no route chip", row.index);
        for (name, other) in [("mute", row.mute), ("solo", row.solo), ("edit", row.edit)] {
            assert!(
                !row.route.intersects(&other),
                "the route chip overlaps {name} on row {}",
                row.index
            );
        }
        assert!(
            !row.route.intersects(&row.name),
            "the route chip overlaps the name on row {}",
            row.index
        );
        assert_eq!(
            row.route.intersection(&row.frame),
            row.route,
            "the route chip escapes its row"
        );
    }
}

#[test]
fn the_name_still_gets_most_of_the_row() {
    // Four controls on a sidebar row is a lot, and a soundfont's name is the
    // longest thing in it. The chip is two characters wide for that reason.
    let m = metrics();
    let l = rack_layout(body(), &m, 1, 0);
    let row = l.rows[0];
    assert!(
        row.name.width > row.frame.width * 0.4,
        "the name got {} of {}",
        row.name.width,
        row.frame.width
    );
}

#[test]
fn clicking_the_route_chip_asks_for_the_menu_and_not_for_the_channel() {
    let m = metrics();
    let l = rack_layout(body(), &m, 3, 0);
    let row = l.rows[1];
    let x = row.route.x + row.route.width / 2.0;
    let y = row.route.y + row.route.height / 2.0;
    assert_eq!(rack_hit(&l, x, y), RackHit::Route(1));
}

#[test]
fn a_row_too_narrow_for_four_controls_still_produces_usable_geometry() {
    let m = metrics();
    for width in [0.0, 20.0, 60.0, 120.0, 248.0] {
        let l = rack_layout(Rect::new(0.0, 0.0, width, 200.0), &m, 3, 0);
        for row in &l.rows {
            for r in [row.name, row.route, row.edit, row.solo, row.mute] {
                assert!(
                    r.width >= 0.0 && r.height >= 0.0,
                    "width {width} gave {r:?}"
                );
                assert!(r.right() <= width + 0.001, "width {width} gave {r:?}");
            }
        }
    }
}

// ------------------------------------------------------------ the menu ---

/// A master and two tracks somebody made — the order `route_names` gives,
/// which is the order the mixer draws them in, master last.
fn names() -> Vec<String> {
    ["Drums", "Reverb", "Master"].map(str::to_string).to_vec()
}

#[test]
fn the_menu_lists_the_master_first_then_every_track_then_a_way_to_make_one() {
    // Master first because it is the default and the commonest answer, and
    // "new track" last because it is the one row that is not a destination.
    let m = metrics();
    let l = rack_layout(body(), &m, 3, 0);
    let menu = route_menu_layout(l.rows[0].route, body(), &m, &names());

    let choices: Vec<RouteChoice> = menu.items.iter().map(|(c, _)| *c).collect();
    assert_eq!(
        choices,
        vec![
            RouteChoice::Master,
            RouteChoice::Track(0),
            RouteChoice::Track(1),
            RouteChoice::New,
        ]
    );
}

#[test]
fn the_menu_names_each_row_after_the_track_it_routes_to() {
    let m = metrics();
    let l = rack_layout(body(), &m, 3, 0);
    let names = names();
    let menu = route_menu_layout(l.rows[0].route, body(), &m, &names);

    assert_eq!(menu.label(RouteChoice::Master, &names), "Master");
    assert_eq!(menu.label(RouteChoice::Track(0), &names), "Drums");
    assert_eq!(menu.label(RouteChoice::Track(1), &names), "Reverb");
    assert_eq!(menu.label(RouteChoice::New, &names), "+ New track");
}

#[test]
fn clicking_a_row_of_the_menu_reports_that_row() {
    let m = metrics();
    let l = rack_layout(body(), &m, 3, 0);
    let menu = route_menu_layout(l.rows[0].route, body(), &m, &names());

    for (choice, rect) in &menu.items {
        let x = rect.x + rect.width / 2.0;
        let y = rect.y + rect.height / 2.0;
        assert_eq!(route_menu_hit(&menu, x, y), Some(*choice));
    }
    assert_eq!(route_menu_hit(&menu, -50.0, -50.0), None);
}

#[test]
fn the_menu_stays_inside_the_panel_it_hangs_in() {
    // A menu dropped from the bottom row of a full rack has to go *up*, or
    // half of it is drawn off the end of the sidebar where it cannot be
    // clicked.
    let m = metrics();
    let bounds = body();
    let l = rack_layout(bounds, &m, 40, 0);
    let last = l.rows.last().expect("a full rack has rows");
    let menu = route_menu_layout(last.route, bounds, &m, &names());

    assert!(!menu.frame.is_empty(), "there is room for it somewhere");
    assert_eq!(
        menu.frame.intersection(&bounds),
        menu.frame,
        "the menu at {:?} escapes {bounds:?}",
        menu.frame
    );
    for (_, rect) in &menu.items {
        assert_eq!(rect.intersection(&bounds), *rect);
    }
}

#[test]
fn a_panel_with_no_room_for_the_menu_produces_no_menu_rather_than_a_sliver() {
    let m = metrics();
    let tiny = Rect::new(0.0, 0.0, 200.0, 10.0);
    let menu = route_menu_layout(Rect::new(0.0, 0.0, 24.0, 18.0), tiny, &m, &names());
    assert!(menu.items.is_empty());
    assert_eq!(route_menu_hit(&menu, 5.0, 5.0), None);
}

#[test]
fn the_chip_says_where_the_channel_goes_in_the_space_it_has() {
    use fontelle_ui::canvas::route_label;
    // The mixer's own numbering, which is what makes "send the drums to 3" a
    // sentence: the master is 0 and everything somebody made counts up.
    assert_eq!(route_label(None, 3), "0");
    assert_eq!(route_label(Some(0), 3), "1");
    assert_eq!(route_label(Some(1), 3), "2");
    // The master's own strip is the last one, and naming it by index is the
    // same answer as `None`.
    assert_eq!(route_label(Some(2), 3), "0");
}

// ------------------------------------------------- the output row's menu ---
//
// > *"if i chose to not route it to master, i wont be hearing my own input but
// > it will still be recording the audio clip."*
//
// The rack's chip says where a *channel* goes and every channel goes
// somewhere, so its menu is the list of destinations. A mixer track's output
// row has one more answer — **nowhere** — and it is the answer that report
// asks for. Which is why the two menus are two functions over one layout
// rather than one function with a flag: a send offered "nowhere" would be a
// row that means "delete this send" and does not.

#[test]
fn a_tracks_output_menu_offers_nowhere_and_the_rest_do_not() {
    let m = metrics();
    let names: Vec<String> = ["Kick", "Bass", "Master"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let chip = Rect::new(20.0, 60.0, 90.0, 18.0);

    let output = output_menu_layout(chip, body(), &m, &names, Some(0));
    let choices: Vec<RouteChoice> = output.items.iter().map(|(c, _)| *c).collect();
    assert!(
        choices.contains(&RouteChoice::Off),
        "a track's output cannot be switched off: {choices:?}"
    );
    assert!(choices.contains(&RouteChoice::Master));
    assert!(
        !choices.contains(&RouteChoice::Track(0)),
        "a track was offered itself"
    );

    let sends = route_menu_layout(chip, body(), &m, &names);
    assert!(
        !sends.items.iter().any(|(c, _)| *c == RouteChoice::Off),
        "a send was offered nowhere to go"
    );
}

#[test]
fn the_off_row_says_so_and_every_row_is_reachable() {
    let m = metrics();
    let names: Vec<String> = ["Kick", "Master"].iter().map(|s| s.to_string()).collect();
    let menu = output_menu_layout(Rect::new(20.0, 60.0, 90.0, 18.0), body(), &m, &names, None);
    for (choice, rect) in &menu.items {
        assert!(
            !menu.label(*choice, &names).is_empty(),
            "{choice:?} draws no words"
        );
        assert_eq!(
            route_menu_hit(&menu, rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
            Some(*choice),
        );
    }
}
