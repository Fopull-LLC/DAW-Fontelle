//! The Matrix page's table (`docs/flopsynth-next.md` §3.4): every cell of a
//! route is a chooser or a switch, rows are added, moved, sorted and removed
//! by their **position in the whole matrix**, and every one of those is one
//! undo. The window sees labels and indexes; the patch is the host's.
//!
//! One of these is a defect: the table's ✕ removed nothing. It asked the
//! host to remove a route *to the depth control's address*, and a depth
//! control is not a destination — so the host found no destination and
//! returned. Nineteen routes on the Grand Piano, none of them removable
//! from the page that lists them.

mod common;

use common::SR;
use fontelle_ui::canvas::FlopsynthPage;
use fontelle_ui::document::{DocumentHost, RouteSort, StudioHost};

/// The studio's opening project: the Grand Piano, nineteen routes.
fn a_piano() -> fontelle_app::Session {
    common::a_session_for(fontelle_app::blank_project(8, 120.0, SR))
}

fn routes(session: &fontelle_app::Session) -> Vec<fontelle_ui::canvas::FlopsynthRoute> {
    session
        .flopsynth(FlopsynthPage::Modulation)
        .expect("Flopsynth's window")
        .routes
}

#[test]
fn every_row_says_its_via_curve_invert_and_bypass_and_the_view_lists_the_choices() {
    let session = a_piano();
    let view = session.flopsynth(FlopsynthPage::Modulation).unwrap();
    let patch = session.selected_patch().unwrap();
    assert_eq!(view.routes.len(), patch.mod_matrix.routes.len());
    for (row, route) in view.routes.iter().zip(&patch.mod_matrix.routes) {
        assert_eq!(row.invert, route.invert);
        assert_eq!(row.bypass, route.bypass);
        assert_eq!(row.via.is_some(), route.via.is_some());
        assert!(
            view.curves.contains(&row.curve),
            "{} is not a curve",
            row.curve
        );
    }
    assert_eq!(
        view.curves,
        ["Linear", "Exponential", "Logarithmic", "S-curve", "Stepped"]
    );
    assert!(view.destinations.iter().any(|d| d == "Filter 1 cutoff"));
    assert!(view.destinations.len() >= 20, "{}", view.destinations.len());
    // The rows' names are drawn from the same lists the choosers offer.
    for row in &view.routes {
        assert!(view.sources.contains(&row.source), "{}", row.source);
        assert!(
            view.destinations.contains(&row.destination),
            "{}",
            row.destination
        );
    }
}

#[test]
fn each_cell_is_set_by_index_and_each_edit_is_one_undo() {
    let mut session = a_piano();
    let view = session.flopsynth(FlopsynthPage::Modulation).unwrap();
    let before = routes(&session);
    let depth = session.undo_depth();
    let lfo = view.sources.iter().position(|s| s == "LFO 2").unwrap();
    let wheel = view.sources.iter().position(|s| s == "Wheel").unwrap();
    let pan = view
        .destinations
        .iter()
        .position(|d| d == "OSC B pan")
        .unwrap();
    let s_curve = view.curves.iter().position(|c| c == "S-curve").unwrap();

    session.set_route_source(0, lfo);
    assert_eq!(routes(&session)[0].source, "LFO 2");
    session.set_route_destination(0, pan);
    assert_eq!(routes(&session)[0].destination, "OSC B pan");
    session.set_route_via(0, Some(wheel));
    assert_eq!(routes(&session)[0].via.as_deref(), Some("Wheel"));
    session.set_route_curve(0, s_curve);
    assert_eq!(routes(&session)[0].curve, "S-curve");
    session.set_route_invert(0, true);
    assert!(routes(&session)[0].invert);
    session.set_route_bypass(0, true);
    assert!(routes(&session)[0].bypass);
    session.set_route_via(0, None);
    assert_eq!(routes(&session)[0].via, None);
    // Seven edits, seven undos, and the row is what it was.
    assert_eq!(session.undo_depth(), depth + 7);
    for _ in 0..7 {
        session.undo();
    }
    assert_eq!(routes(&session), before);
    // And the patch heard it: a bypassed route is written as one.
    session.set_route_bypass(3, true);
    assert!(session.selected_patch().unwrap().mod_matrix.routes[3].bypass);
    // A row or a choice that is not there does nothing, and is not an undo.
    let depth = session.undo_depth();
    session.set_route_source(99, lfo);
    session.set_route_curve(0, 99);
    assert_eq!(session.undo_depth(), depth);
}

#[test]
fn rows_are_removed_added_moved_and_sorted_by_position() {
    let mut session = a_piano();
    let before = routes(&session);
    let count = before.len();
    assert!(count >= 5);

    // The ✕ on row 1 takes row 1 — the defect.
    session.remove_route_row(1);
    let after = routes(&session);
    assert_eq!(after.len(), count - 1);
    assert_eq!(after[0], before[0]);
    assert_eq!(after[1], before[2]);
    session.undo();
    assert_eq!(routes(&session), before);

    // `+` adds a row at the end, audible at once (§8.4's half depth), from
    // the first LFO to the first filter's cutoff.
    session.add_route_row();
    let added = routes(&session);
    assert_eq!(added.len(), count + 1);
    let new = added.last().unwrap();
    assert_eq!(new.source, "LFO 1");
    assert_eq!(new.destination, "Filter 1 cutoff");
    assert!((new.depth - 0.5).abs() < 1e-6);
    assert_eq!(new.curve, "Linear");
    session.undo();
    assert_eq!(routes(&session), before);

    // Moved by its grip: row 0 dropped on row 3 goes before the row that
    // was 3; the last dropped on row 0 goes first.
    session.move_route(0, 3);
    let moved = routes(&session);
    assert_eq!(moved[0], before[1]);
    assert_eq!(moved[1], before[2]);
    assert_eq!(moved[2], before[0]);
    assert_eq!(moved[3], before[3]);
    session.undo();
    session.move_route(count - 1, 0);
    let moved = routes(&session);
    assert_eq!(moved[0], before[count - 1]);
    assert_eq!(moved[1], before[0]);
    session.undo();
    // Dropped on itself, or on the row after it (the same place): nothing,
    // and no undo. Dropped below the last row: last.
    let depth = session.undo_depth();
    session.move_route(2, 2);
    session.move_route(2, 3);
    assert_eq!(routes(&session), before);
    assert_eq!(session.undo_depth(), depth);
    session.move_route(2, count);
    assert_eq!(routes(&session).last(), Some(&before[2]));
    session.undo();
    assert_eq!(routes(&session), before);

    // Sorted by source: the badges' order, envelopes first; stable, so two
    // routes from one source keep their order. By destination likewise.
    let view = session.flopsynth(FlopsynthPage::Modulation).unwrap();
    let source_rank = |name: &str| view.sources.iter().position(|s| s == name).unwrap();
    let dest_rank = |name: &str| view.destinations.iter().position(|d| d == name).unwrap();
    session.sort_routes(RouteSort::Source);
    let sorted = routes(&session);
    assert!(
        sorted
            .windows(2)
            .all(|w| source_rank(&w[0].source) <= source_rank(&w[1].source))
    );
    let mut expected = before.clone();
    expected.sort_by_key(|r| source_rank(&r.source));
    assert_eq!(sorted, expected, "stable");
    session.undo();
    assert_eq!(routes(&session), before);
    session.sort_routes(RouteSort::Destination);
    let sorted = routes(&session);
    assert!(
        sorted
            .windows(2)
            .all(|w| dest_rank(&w[0].destination) <= dest_rank(&w[1].destination))
    );
    session.undo();
    assert_eq!(routes(&session), before);
}
