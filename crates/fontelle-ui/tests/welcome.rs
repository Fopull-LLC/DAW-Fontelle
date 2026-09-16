//! The start menu (TDD §18's first-run half, and every launch after it).
//!
//! > *"you open it it checks for updates and you have the option to upgrade
//! > if theres an update, or open an existing project from your recent
//! > projects or make a new project. this will be the start menu of the
//! > software which is just the panel that helps you get where you need to
//! > go, has the logo, and should also have a section somewhere marking it
//! > as a open source product of Fopull LLC"*
//!
//! The menu is a scene the window draws over the studio until one of its
//! buttons is pressed. Like every canvas here it is a pure view-model: a
//! layout from a rectangle and a count, a hit from a point, and a request
//! back to the window. Nothing in it opens a file or a socket.

use fontelle_ui::UpdateStatus;
use fontelle_ui::canvas::{
    REPOSITORY_URL, WEBSITE_URL, WelcomeHit, WelcomeLayout, update_line, welcome_hit,
    welcome_layout,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn window() -> Rect {
    Rect::new(0.0, 0.0, 1280.0, 720.0)
}

fn within(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x - 0.01
        && inner.y >= outer.y - 0.01
        && inner.x + inner.width <= outer.x + outer.width + 0.01
        && inner.y + inner.height <= outer.y + outer.height + 0.01
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

/// Every rectangle a person can press, in the order they are drawn.
fn pressables(layout: &WelcomeLayout) -> Vec<Rect> {
    let mut all = vec![
        layout.new_button,
        layout.open_button,
        layout.website,
        layout.repository,
    ];
    all.extend(layout.update_button);
    all.extend(layout.rows.iter().map(|row| row.frame));
    all
}

#[test]
fn everything_sits_inside_the_window_and_nothing_pressable_overlaps() {
    for (w, h) in [(1280.0, 720.0), (900.0, 560.0), (640.0, 400.0)] {
        let window = Rect::new(0.0, 0.0, w, h);
        let layout = welcome_layout(window, &metrics(), 5, true);
        let all: Vec<Rect> = pressables(&layout)
            .into_iter()
            .chain([
                layout.logo,
                layout.title,
                layout.version,
                layout.update,
                layout.footer,
            ])
            .collect();
        for rect in &all {
            assert!(within(*rect, window), "{rect:?} is outside {w}x{h}");
            assert!(!rect.is_empty(), "{rect:?} is empty at {w}x{h}");
        }
        let pressable = pressables(&layout);
        for (i, a) in pressable.iter().enumerate() {
            for b in &pressable[i + 1..] {
                assert!(!overlaps(*a, *b), "{a:?} overlaps {b:?} at {w}x{h}");
            }
        }
    }
}

#[test]
fn the_card_is_centred_in_the_window() {
    let layout = welcome_layout(window(), &metrics(), 3, false);
    let (cx, _) = centre(layout.frame);
    assert!((cx - 640.0).abs() < 1.0, "{:?}", layout.frame);
}

#[test]
fn one_row_per_recent_project_each_with_its_own_forget_button() {
    let layout = welcome_layout(window(), &metrics(), 4, false);
    assert_eq!(layout.rows.len(), 4);
    assert!(layout.empty_recent.is_none());
    for row in &layout.rows {
        assert!(within(row.forget, row.frame), "the × lives in its row");
        assert!(
            row.forget.x > row.frame.x + row.frame.width / 2.0,
            "at the right-hand end"
        );
    }
    // Rows follow one another downwards, newest at the top.
    for pair in layout.rows.windows(2) {
        assert!(pair[1].frame.y >= pair[0].frame.y + pair[0].frame.height);
    }
}

#[test]
fn with_nothing_recent_the_list_says_so_instead_of_being_blank() {
    let layout = welcome_layout(window(), &metrics(), 0, false);
    assert!(layout.rows.is_empty());
    let empty = layout
        .empty_recent
        .expect("a line saying there is nothing yet");
    assert!(within(empty, layout.frame));
}

#[test]
fn a_long_list_is_cut_to_what_fits_rather_than_run_off_the_card() {
    // A short window: the settings cap the list at eight, but the card does
    // not have to hold eight rows, and a row drawn over the buttons is worse
    // than one not drawn.
    let short = Rect::new(0.0, 0.0, 900.0, 400.0);
    let layout = welcome_layout(short, &metrics(), 8, true);
    for row in &layout.rows {
        assert!(within(row.frame, layout.frame));
        assert!(!overlaps(row.frame, layout.new_button));
        assert!(!overlaps(row.frame, layout.footer));
    }
}

#[test]
fn a_message_has_a_line_of_its_own_just_above_the_buttons() {
    // "That project is not there any more" has to land somewhere on the
    // card, and the card is the only thing on screen.
    let layout = welcome_layout(window(), &metrics(), 3, true);
    assert!(within(layout.message, layout.frame));
    assert!(layout.message.y + layout.message.height <= layout.new_button.y);
    assert!(!overlaps(layout.message, layout.update_button.unwrap()));
}

#[test]
fn the_update_button_is_only_there_when_asked_for() {
    assert!(
        welcome_layout(window(), &metrics(), 0, false)
            .update_button
            .is_none()
    );
    let with = welcome_layout(window(), &metrics(), 0, true);
    let button = with.update_button.expect("asked for");
    // Beside the status line, under the version: the two are read together.
    assert!(button.y >= with.version.y + with.version.height);
}

#[test]
fn every_pressable_thing_answers_to_a_point_inside_it() {
    let layout = welcome_layout(window(), &metrics(), 3, true);
    let at = |r: Rect| {
        let (x, y) = centre(r);
        welcome_hit(&layout, x, y)
    };
    assert_eq!(at(layout.new_button), Some(WelcomeHit::NewProject));
    assert_eq!(at(layout.open_button), Some(WelcomeHit::OpenProject));
    assert_eq!(at(layout.update_button.unwrap()), Some(WelcomeHit::Update));
    assert_eq!(at(layout.website), Some(WelcomeHit::Website));
    assert_eq!(at(layout.repository), Some(WelcomeHit::Repository));
    assert_eq!(at(layout.rows[2].frame), Some(WelcomeHit::Recent(2)));
    assert_eq!(at(layout.rows[1].forget), Some(WelcomeHit::Forget(1)));
    // The name half of a row is the row, not the ×.
    let name = Rect::new(
        layout.rows[1].frame.x + 4.0,
        layout.rows[1].frame.y,
        8.0,
        layout.rows[1].frame.height,
    );
    assert_eq!(at(name), Some(WelcomeHit::Recent(1)));
    // The logo, the title, the footer's own words: nothing.
    assert_eq!(at(layout.logo), None);
    assert_eq!(at(layout.title), None);
    assert_eq!(welcome_hit(&layout, -1.0, -1.0), None);
}

#[test]
fn the_status_line_says_what_the_check_found_and_when_there_is_something_to_press() {
    let line = |status: &UpdateStatus| update_line(status, "0.1.0");
    assert_eq!(line(&UpdateStatus::Unchecked).1, None);
    assert!(
        line(&UpdateStatus::Checking)
            .0
            .to_lowercase()
            .contains("checking")
    );
    assert_eq!(line(&UpdateStatus::Checking).1, None);
    assert!(
        line(&UpdateStatus::UpToDate)
            .0
            .to_lowercase()
            .contains("up to date")
    );
    assert_eq!(line(&UpdateStatus::UpToDate).1, None);

    let available = line(&UpdateStatus::Available {
        version: "0.2.0".into(),
    });
    assert!(available.0.contains("0.2.0"), "{}", available.0);
    assert!(
        available
            .1
            .is_some_and(|b| b.to_lowercase().contains("install"))
    );

    let downloading = line(&UpdateStatus::Downloading {
        version: "0.2.0".into(),
        done: 0,
        total: None,
    });
    assert!(downloading.0.contains("0.2.0"));
    assert_eq!(downloading.1, None, "nothing to press while it downloads");

    let installed = line(&UpdateStatus::Installed {
        version: "0.2.0".into(),
    });
    assert!(
        installed.0.to_lowercase().contains("restart"),
        "{}",
        installed.0
    );
    assert_eq!(installed.1, None);

    let failed = line(&UpdateStatus::Failed("no route to host".into()));
    assert!(failed.0.contains("no route to host"));
    assert!(
        failed
            .1
            .is_some_and(|b| b.to_lowercase().contains("release")),
        "a way to get it by hand"
    );

    let off = line(&UpdateStatus::Off);
    assert!(off.0.to_lowercase().contains("off"), "{}", off.0);
    assert_eq!(off.1, None);
}

#[test]
fn the_footer_names_the_company_and_the_links_are_real_addresses() {
    assert_eq!(WEBSITE_URL, "https://fopull.com");
    assert!(REPOSITORY_URL.starts_with("https://github.com/Fopull-LLC/"));
    let layout = welcome_layout(window(), &metrics(), 0, false);
    // The footer is the last thing on the card and the links are in it.
    assert!(within(layout.website, layout.footer));
    assert!(within(layout.repository, layout.footer));
    assert!(layout.footer.y + layout.footer.height <= layout.frame.y + layout.frame.height + 0.01);
}

/// *"make it so theres a progress bar when installing an update"*: while
/// the archive comes down the line says how far, and the slot the offer
/// button occupied holds a bar — full to the fraction when the size is
/// known, and a marching indeterminate one when it is not.
#[test]
fn a_download_in_progress_says_how_far_and_has_a_bar() {
    use fontelle_ui::canvas::update_progress;
    let line = |status: &UpdateStatus| update_line(status, "0.1.0");
    let known = UpdateStatus::Downloading {
        version: "0.2.0".into(),
        done: 3_100_000,
        total: Some(12_400_000),
    };
    let (text, button) = line(&known);
    assert!(text.contains("3.1"), "{text}");
    assert!(text.contains("12.4"), "{text}");
    assert!(text.contains("MB"), "{text}");
    assert_eq!(button, None);
    let fraction = update_progress(&known).expect("a bar while downloading");
    assert!((fraction.unwrap() - 0.25).abs() < 0.01, "{fraction:?}");

    let unknown = UpdateStatus::Downloading {
        version: "0.2.0".into(),
        done: 3_100_000,
        total: None,
    };
    let (text, _) = line(&unknown);
    assert!(text.contains("3.1"), "{text}");
    assert_eq!(update_progress(&unknown), Some(None), "indeterminate");
    assert_eq!(update_progress(&UpdateStatus::UpToDate), None);

    // The bar takes the button's slot: the layout has a rectangle for it.
    let layout = fontelle_ui::canvas::welcome_layout(
        Rect::new(0.0, 0.0, 1200.0, 800.0),
        &metrics(),
        0,
        true,
    );
    assert!(layout.update_button.is_some());
}

// ------------------------------------------------------------- the help icon ---

#[test]
fn the_card_has_a_help_button_in_its_top_corner_and_it_is_pressable() {
    // *"should be accessible from the start screen with a new ? icon"*. In
    // the top right, out of the way of the logo and the list, the size of a
    // row so it reads as a button rather than a stray glyph.
    let l = welcome_layout(window(), &metrics(), 3, true);
    assert!(!l.help.is_empty(), "no help button");
    assert!(within(l.help, l.frame), "the help button is off the card");
    assert!(
        l.help.right() >= l.frame.right() - 40.0,
        "the help button is not in the right-hand corner: {:?} on {:?}",
        l.help,
        l.frame
    );
    assert!(
        l.help.y <= l.frame.y + 40.0,
        "the help button is not at the top: {:?} on {:?}",
        l.help,
        l.frame
    );
    for other in [
        l.logo,
        l.title,
        l.version,
        l.recent_heading,
        l.new_button,
        l.open_button,
    ] {
        assert!(
            !overlaps(l.help, other),
            "the help button is over {other:?}"
        );
    }
    for row in &l.rows {
        assert!(
            !overlaps(l.help, row.frame),
            "the help button is over a recent row"
        );
    }
    let (x, y) = (
        l.help.x + l.help.width / 2.0,
        l.help.y + l.help.height / 2.0,
    );
    assert_eq!(welcome_hit(&l, x, y), Some(WelcomeHit::Help));
}
