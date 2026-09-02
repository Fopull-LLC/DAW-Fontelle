//! Browsing projects from inside the window (TDD §17.1, §17.3).
//!
//! Reported from using the window: *"I should be able to make new projects
//! from within the app, browse my projects within the app, set my projects
//! folder if not already set, change it if it is."*
//!
//! The browser panel grows a second mode rather than the window growing a
//! second panel. Two reasons, and both are about the shape of a session: you
//! reach for a project at the start and the end and for a soundfont all the
//! way through, so they are never wanted at once; and a window that changes
//! shape depending on what you are doing is one you have to re-learn.
//!
//! The folder half — what a projects folder is, and why there is no default —
//! is `fontelle-app/tests/projects.rs`.

use fontelle_ui::canvas::{BrowserHit, BrowserMode, browser_hit, browser_layout_for};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn body() -> Rect {
    Rect::new(8.0, 40.0, 248.0, 420.0)
}

fn sounds(files: usize, presets: usize) -> fontelle_ui::canvas::BrowserLayout {
    browser_layout_for(
        body(),
        &metrics(),
        BrowserMode::Sounds,
        files,
        presets,
        0,
        0,
    )
}

fn projects(count: usize) -> fontelle_ui::canvas::BrowserLayout {
    browser_layout_for(body(), &metrics(), BrowserMode::Projects, count, 0, 0, 0)
}

// --------------------------------------------------------------- the tabs ---

#[test]
fn the_panel_has_a_tab_for_each_mode_and_they_do_not_overlap() {
    let l = sounds(4, 4);
    assert!(!l.sounds_tab.is_empty());
    assert!(!l.projects_tab.is_empty());
    assert!(!l.sounds_tab.intersects(&l.projects_tab));
    for tab in [l.sounds_tab, l.projects_tab] {
        assert_eq!(tab.intersection(&l.body), tab, "{tab:?} escapes the panel");
    }
}

#[test]
fn clicking_a_tab_asks_for_that_mode() {
    let l = sounds(4, 4);
    let mid = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
    let (x, y) = mid(l.sounds_tab);
    assert_eq!(browser_hit(&l, x, y), BrowserHit::Mode(BrowserMode::Sounds));
    let (x, y) = mid(l.projects_tab);
    assert_eq!(
        browser_hit(&l, x, y),
        BrowserHit::Mode(BrowserMode::Projects)
    );
}

#[test]
fn the_tabs_sit_above_the_search_box_and_not_on_it() {
    // The search filters whichever list is showing, so it belongs *under* the
    // switch that decides which list that is.
    let l = sounds(4, 4);
    assert!(l.sounds_tab.bottom() <= l.search.y + 0.001);
}

// ------------------------------------------------------- the projects list ---

#[test]
fn projects_mode_gives_the_whole_list_area_to_one_list() {
    // There is no second list in this mode — a project has no presets inside
    // it — so splitting the area in two would leave half the panel empty.
    let l = projects(12);
    assert!(l.presets.is_empty(), "no second list: {:?}", l.presets);
    assert!(l.preset_rows.is_empty());
    assert!(
        l.files.height > sounds(12, 12).files.height,
        "and the one list gets the room the other used to take"
    );
}

#[test]
fn a_project_row_reports_itself() {
    let l = projects(6);
    assert!(!l.file_rows.is_empty());
    let (index, rect) = l.file_rows[2];
    let hit = browser_hit(&l, rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    assert_eq!(hit, BrowserHit::File(index));
}

#[test]
fn the_footer_buttons_are_the_same_two_places_in_both_modes() {
    // They mean different things — "open the bank" against "open the projects
    // folder" — but a button that moves when you switch tabs is one you have
    // to find again every time.
    let a = sounds(4, 4);
    let b = projects(4);
    assert_eq!(a.open_folder, b.open_folder);
    assert_eq!(a.choose_folder, b.choose_folder);
}

#[test]
fn projects_mode_has_a_way_to_make_one() {
    // The whole reason somebody opens this tab on a first run.
    let l = projects(3);
    assert!(!l.new_project.is_empty());
    assert_eq!(
        l.new_project.intersection(&l.body),
        l.new_project,
        "the new-project button escapes the panel"
    );
    let hit = browser_hit(
        &l,
        l.new_project.x + l.new_project.width / 2.0,
        l.new_project.y + l.new_project.height / 2.0,
    );
    assert_eq!(hit, BrowserHit::NewProject);
}

#[test]
fn sounds_mode_has_no_new_project_button() {
    // A control that does nothing in the mode you are in is worse than one
    // that is not there.
    let l = sounds(4, 4);
    assert!(l.new_project.is_empty());
    assert_ne!(
        browser_hit(&l, l.body.x + 4.0, l.body.bottom() - 4.0),
        BrowserHit::NewProject
    );
}

#[test]
fn a_panel_too_small_for_any_of_it_yields_empty_rects_never_negative_ones() {
    for (w, h) in [(0.0, 0.0), (40.0, 20.0), (248.0, 30.0), (60.0, 420.0)] {
        for mode in [BrowserMode::Sounds, BrowserMode::Projects] {
            let l = browser_layout_for(Rect::new(0.0, 0.0, w, h), &metrics(), mode, 8, 8, 0, 0);
            for r in [
                l.sounds_tab,
                l.projects_tab,
                l.search,
                l.files,
                l.presets,
                l.status,
                l.open_folder,
                l.choose_folder,
                l.new_project,
            ] {
                assert!(r.width >= 0.0 && r.height >= 0.0, "{w}x{h} gave {r:?}");
            }
        }
    }
}

#[test]
fn nothing_in_the_footer_is_drawn_on_top_of_anything_else() {
    // Reported from a screenshot of the Projects tab: the status line — *"no
    // projects folder yet — ..."* — was drawn straight across the "New" and
    // "Export..." buttons, because both were placed at the same `y`. Two
    // captions in the same pixels is not a panel you can read, and neither of
    // the buttons underneath looked pressable.
    //
    // Stated as "no two rectangles overlap" rather than as "new_project sits
    // at status.y - height", because the second is the arithmetic that was
    // already wrong once and the first is what a person actually sees.
    let l = projects(3);
    let named = [
        ("sounds tab", l.sounds_tab),
        ("projects tab", l.projects_tab),
        ("search", l.search),
        ("list", l.files),
        ("status", l.status),
        ("new project", l.new_project),
        ("export", l.export),
        ("open folder", l.open_folder),
        ("choose folder", l.choose_folder),
    ];
    for (i, (a_name, a)) in named.iter().enumerate() {
        for (b_name, b) in named.iter().skip(i + 1) {
            if a.is_empty() || b.is_empty() {
                continue;
            }
            assert!(
                !a.intersects(b),
                "the {a_name} {a:?} is drawn over the {b_name} {b:?}"
            );
        }
    }
}

#[test]
fn the_status_line_is_readable_in_both_modes() {
    // It is the line that says where the folder is, or what just went wrong.
    // A status line with no height is a message nobody ever sees, and one
    // sharing its row with a button is a message nobody can read.
    for l in [sounds(4, 4), projects(3)] {
        assert!(!l.status.is_empty(), "the status line has nowhere to go");
        assert!(
            !l.status.intersects(&l.new_project),
            "the status line and the new-project button share pixels"
        );
        assert!(
            !l.status.intersects(&l.export),
            "the status line and the export button share pixels"
        );
        assert!(!l.status.intersects(&l.open_folder));
        assert!(!l.status.intersects(&l.choose_folder));
    }
}

#[test]
fn the_project_list_stops_above_everything_under_it() {
    // The list is the tallest thing in the panel, so it is the one that has to
    // give way — a row drawn under the status line hit-tests as a row and
    // reads as a caption cut in half.
    let l = projects(40);
    assert!(!l.new_project.is_empty());
    assert!(
        l.files.bottom() <= l.new_project.y + 0.001,
        "the list {:?} runs under the new-project row {:?}",
        l.files,
        l.new_project
    );
    for (_, row) in &l.file_rows {
        assert!(
            row.bottom() <= l.files.bottom() + 0.001,
            "row {row:?} escapes the list {:?}",
            l.files
        );
    }
}

// ------------------------------------------------- which folder, exactly ---

#[test]
fn a_folder_button_says_which_folder_it_is_about() {
    // Reported from using the window: *"if I select change to set my projects
    // folder and I select a folder it actually just changes my soundfonts
    // folder"*. The click handler branched on the panel's mode for "Open
    // folder" and not for "Change...", so the Projects tab's own button
    // replaced the soundfont bank and left the projects folder unset —
    // which makes project management impossible, since there is nowhere for a
    // new project to go.
    //
    // Stated as "the hit carries the mode" rather than as "the handler
    // branches", because a branch is what was already forgotten once. A hit
    // that names its own folder cannot be answered with the other one.
    for (mode, l) in [
        (BrowserMode::Sounds, sounds(4, 4)),
        (BrowserMode::Projects, projects(4)),
    ] {
        let mid = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
        let (x, y) = mid(l.choose_folder);
        assert_eq!(
            browser_hit(&l, x, y),
            BrowserHit::ChooseFolder(mode),
            "\"Change...\" in {mode:?} mode has to be about {mode:?}"
        );
        let (x, y) = mid(l.open_folder);
        assert_eq!(
            browser_hit(&l, x, y),
            BrowserHit::OpenFolder(mode),
            "\"Open folder\" in {mode:?} mode has to be about {mode:?}"
        );
    }
}

#[test]
fn the_layout_remembers_which_mode_it_was_built_for() {
    // The hit above can only name a folder because the geometry knows which
    // list it laid out. Everything else about the panel already varies by
    // mode; this is the field that makes it answerable.
    assert_eq!(sounds(4, 4).mode, BrowserMode::Sounds);
    assert_eq!(projects(4).mode, BrowserMode::Projects);
}
