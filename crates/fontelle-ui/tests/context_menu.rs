//! The right-click menu's geometry.

use fontelle_ui::canvas::{MenuEntry, context_menu_hit, context_menu_layout};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn font_size() -> f32 {
    Theme::dark_default().font.size
}

fn menu(at: (f32, f32), bounds: Rect, labels: &[&str]) -> fontelle_ui::canvas::ContextMenu {
    context_menu_layout(
        at,
        bounds,
        &metrics(),
        font_size(),
        labels.iter().map(|l| MenuEntry::new(*l)).collect(),
    )
}

#[test]
fn a_menu_opens_below_and_right_of_the_pointer() {
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let menu = menu((100.0, 100.0), bounds, &["Duplicate", "Delete"]);
    assert_eq!(menu.rows.len(), 2);
    assert!(menu.frame.x >= 100.0 && menu.frame.y >= 100.0);
    assert!(menu.rows[0].y < menu.rows[1].y, "in the order given");
}

#[test]
fn a_menu_near_an_edge_folds_back_inside_the_window() {
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let menu = menu((795.0, 595.0), bounds, &["Duplicate", "Delete", "Rename"]);
    assert!(
        menu.frame.right() <= bounds.right() + 0.01,
        "not off the right: {:?}",
        menu.frame
    );
    assert!(
        menu.frame.bottom() <= bounds.bottom() + 0.01,
        "not off the bottom: {:?}",
        menu.frame
    );
}

#[test]
fn a_menu_is_wide_enough_for_its_longest_entry() {
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let short = menu((0.0, 0.0), bounds, &["Cut"]);
    let long = menu(
        (0.0, 0.0),
        bounds,
        &["Create automation clip for this control"],
    );
    assert!(
        long.frame.width > short.frame.width,
        "a long caption gets a wider menu, {} against {}",
        long.frame.width,
        short.frame.width
    );
}

#[test]
fn clicking_a_row_chooses_it() {
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let menu = menu((40.0, 40.0), bounds, &["Duplicate", "Delete"]);
    let row = menu.rows[1];
    assert_eq!(
        context_menu_hit(&menu, row.x + 4.0, row.y + row.height / 2.0),
        Some(1)
    );
    assert_eq!(
        context_menu_hit(&menu, row.x - 40.0, row.y),
        None,
        "a press off the menu chooses nothing"
    );
}

/// A greyed entry says why something is not available and does nothing.
#[test]
fn a_disabled_row_cannot_be_chosen() {
    let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
    let menu = context_menu_layout(
        (40.0, 40.0),
        bounds,
        &metrics(),
        font_size(),
        vec![
            MenuEntry::disabled("Delete lane"),
            MenuEntry::new("Rename lane"),
        ],
    );
    let row = menu.rows[0];
    assert_eq!(context_menu_hit(&menu, row.x + 4.0, row.y + 2.0), None);
    assert!(
        menu.frame.contains(row.x + 4.0, row.y + 2.0),
        "it is still inside the menu, so the press does not dismiss it either"
    );
}

/// A panel with no room for the whole menu draws none of it rather than a
/// sliver listing two of five things.
#[test]
fn a_menu_that_will_not_fit_is_not_drawn() {
    let bounds = Rect::new(0.0, 0.0, 800.0, 10.0);
    let menu = menu((0.0, 0.0), bounds, &["One", "Two", "Three", "Four"]);
    assert!(menu.is_empty());
    assert_eq!(context_menu_hit(&menu, 1.0, 1.0), None);
}
