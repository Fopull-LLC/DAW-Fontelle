//! The settings tab (TDD §14.3, §18).
//!
//! Reported from playing a keyboard: *"the options [should] also be
//! configurable in a settings tab so I can adjust like my velocity for example
//! on my midi input, since every single input device will register
//! differently."*
//!
//! It is a third **mode of the browser panel**, beside Sounds and Projects,
//! rather than a fourth panel or a dialog. The reason is the one the second
//! mode was added for: this is a thing you reach for occasionally and never at
//! the same time as the other two, and a window that grows a panel when you
//! open a preference is one you have to re-learn.
//!
//! Every row is a **name and a value**, which is exactly a `LibraryEntry` —
//! so the list, its virtualisation, its rows and its hit-testing are the ones
//! the other two modes already use, and this file is about what is *different*
//! rather than about a second list widget.

use fontelle_ui::canvas::{BrowserHit, BrowserMode, browser_hit, browser_layout_for};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn body() -> Rect {
    Rect::new(8.0, 40.0, 248.0, 420.0)
}

fn mode(mode: BrowserMode, rows: usize) -> fontelle_ui::canvas::BrowserLayout {
    browser_layout_for(body(), &metrics(), mode, rows, 0, 0, 0)
}

fn settings(rows: usize) -> fontelle_ui::canvas::BrowserLayout {
    mode(BrowserMode::Settings, rows)
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

// ----------------------------------------------------------------- the tab ---

#[test]
fn the_panel_has_three_tabs_and_none_of_them_overlap() {
    let l = settings(6);
    let tabs = [
        ("sounds", l.tab(BrowserMode::Sounds)),
        ("projects", l.tab(BrowserMode::Projects)),
        ("settings", l.tab(BrowserMode::Settings)),
    ];
    for (name, tab) in tabs {
        assert!(!tab.is_empty(), "the {name} tab has nowhere to go");
        assert_eq!(
            tab.intersection(&l.body),
            tab,
            "the {name} tab {tab:?} escapes the panel"
        );
    }
    for (i, (a_name, a)) in tabs.iter().enumerate() {
        for (b_name, b) in tabs.iter().skip(i + 1) {
            assert!(
                !a.intersects(b),
                "the {a_name} tab is over the {b_name} tab"
            );
        }
    }
}

#[test]
fn clicking_the_settings_tab_asks_for_that_mode() {
    let l = mode(BrowserMode::Sounds, 4);
    let (x, y) = centre(l.tab(BrowserMode::Settings));
    assert_eq!(
        browser_hit(&l, x, y),
        BrowserHit::Mode(BrowserMode::Settings)
    );
}

#[test]
fn the_tabs_are_in_the_same_places_whichever_one_is_open() {
    // A tab that moves when you press it is one you have to find again.
    let a = mode(BrowserMode::Sounds, 4);
    let b = mode(BrowserMode::Projects, 4);
    let c = settings(4);
    for l in [&b, &c] {
        for mode in BrowserMode::ALL {
            assert_eq!(a.tab(mode), l.tab(mode), "{mode:?}");
        }
    }
}

// -------------------------------------------------------------- the panel ---

#[test]
fn a_setting_row_reports_itself_the_way_every_other_row_does() {
    // Which is what lets the click handler be "activate row N" in all three
    // modes rather than three handlers that drift apart.
    let l = settings(6);
    assert!(!l.file_rows.is_empty());
    let (index, rect) = l.file_rows[3];
    assert_eq!(
        browser_hit(&l, rect.x + 4.0, rect.y + 2.0),
        BrowserHit::File(index)
    );
}

#[test]
fn there_is_no_search_box_over_a_list_of_six_things() {
    // The box filters whichever list is showing, and a settings list is short
    // enough to read. A field that does nothing is worse than no field: it
    // takes the keyboard when you click it and then says nothing back.
    let l = settings(6);
    assert!(l.search.is_empty(), "a search box at {:?}", l.search);
    assert!(
        l.files.height > mode(BrowserMode::Projects, 6).files.height,
        "and the row it would have taken goes to the list"
    );
}

#[test]
fn there_is_no_second_list_and_nothing_to_make_a_project_with() {
    let l = settings(6);
    assert!(l.presets.is_empty());
    assert!(l.preset_rows.is_empty());
    assert!(l.new_project.is_empty(), "a New button in the settings tab");
    assert!(l.export.is_empty(), "an Export button in the settings tab");
}

#[test]
fn the_one_folder_worth_opening_from_here_is_the_one_the_settings_live_in() {
    // "Change..." has no meaning in this mode — there is no folder being
    // browsed — so it is not drawn, and the row goes to the button that does
    // mean something.
    let l = settings(6);
    assert!(l.choose_folder.is_empty(), "a Change button with no folder");
    assert!(!l.open_folder.is_empty());
    let (x, y) = centre(l.open_folder);
    assert_eq!(
        browser_hit(&l, x, y),
        BrowserHit::OpenFolder(BrowserMode::Settings)
    );
}

#[test]
fn the_status_line_still_has_somewhere_to_be() {
    let l = settings(6);
    assert!(!l.status.is_empty());
    assert!(!l.status.intersects(&l.open_folder));
    assert!(!l.status.intersects(&l.files));
}

#[test]
fn nothing_in_the_settings_tab_is_drawn_on_top_of_anything_else() {
    let l = settings(40);
    let named = [
        ("sounds tab", l.tab(BrowserMode::Sounds)),
        ("projects tab", l.tab(BrowserMode::Projects)),
        ("settings tab", l.tab(BrowserMode::Settings)),
        ("list", l.files),
        ("status", l.status),
        ("open folder", l.open_folder),
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
    for (_, row) in &l.file_rows {
        assert!(
            row.bottom() <= l.files.bottom() + 0.001,
            "row {row:?} escapes"
        );
    }
}

#[test]
fn a_panel_too_small_for_any_of_it_yields_empty_rects_never_negative_ones() {
    for (w, h) in [(0.0, 0.0), (40.0, 20.0), (248.0, 30.0), (60.0, 420.0)] {
        let l = browser_layout_for(
            Rect::new(0.0, 0.0, w, h),
            &metrics(),
            BrowserMode::Settings,
            8,
            8,
            0,
            0,
        );
        for r in [
            l.tab(BrowserMode::Sounds),
            l.tab(BrowserMode::Projects),
            l.tab(BrowserMode::Settings),
            l.search,
            l.files,
            l.presets,
            l.status,
            l.open_folder,
            l.choose_folder,
        ] {
            assert!(r.width >= 0.0 && r.height >= 0.0, "{w}x{h} gave {r:?}");
        }
    }
}

// -------------------------------------------- rows are controls, not steps ---
//
// A settings row draws a real control on its right — a groove a number is
// dragged along, a pill a switch is, a caret a choice drops from — rather than
// a value you click to step. This is pure geometry: where the control sits and
// where along its groove a fraction lands, so a drag and the fill agree.

use fontelle_ui::canvas::{setting_control_rect, setting_slider_at, setting_slider_x_of};

fn a_row() -> Rect {
    settings(6).file_rows[1].1
}

#[test]
fn a_settings_control_sits_on_the_right_of_its_row() {
    let row = a_row();
    let c = setting_control_rect(row, &metrics());
    assert!(!c.is_empty(), "the control has nowhere to go");
    assert_eq!(c.intersection(&row), c, "the control escapes its row");
    assert!(c.x > row.x + row.width / 2.0, "the control is on the right");
    assert!(c.right() <= row.right() + 0.01);
}

#[test]
fn a_sliders_fraction_and_its_x_are_two_ways_round_the_same_point() {
    let c = setting_control_rect(a_row(), &metrics());
    for f in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let x = setting_slider_x_of(c, f);
        assert!((setting_slider_at(c, x) - f).abs() < 0.001, "at {f}");
    }
    // The left end is empty and the right end is full.
    assert_eq!(setting_slider_at(c, c.x), 0.0);
    assert!(setting_slider_at(c, c.right()) >= 0.999);
    // Off either end is held inside 0..=1, so a drag past the groove does not
    // set a value past its ends.
    assert_eq!(setting_slider_at(c, c.x - 50.0), 0.0);
    assert_eq!(setting_slider_at(c, c.right() + 50.0), 1.0);
}

#[test]
fn a_control_on_a_zero_width_row_is_empty_not_negative() {
    let c = setting_control_rect(Rect::new(0.0, 0.0, 0.0, 0.0), &metrics());
    assert!(c.width >= 0.0 && c.height >= 0.0);
    assert_eq!(setting_slider_at(c, 5.0), 0.0);
}

// ------------------------------------------------ a row you type into (F47) ---

/// What a press on a settings row does, decided by its control and nothing
/// else. A text row is typed into — the prompt a project is named in,
/// seeded with what the row holds — and a press never steps it.
#[test]
fn a_text_row_is_typed_into_seeded_with_what_it_holds() {
    use fontelle_ui::canvas::{SettingControl, SettingPress, setting_press};
    assert_eq!(
        setting_press(&SettingControl::Text { text: "Bob".into() }),
        SettingPress::Type("Bob".into())
    );
    assert_eq!(
        setting_press(&SettingControl::Heading),
        SettingPress::Nothing
    );
    assert_eq!(setting_press(&SettingControl::Button), SettingPress::Act);
    assert_eq!(
        setting_press(&SettingControl::Switch { on: true }),
        SettingPress::Flip
    );
    assert_eq!(
        setting_press(&SettingControl::Slider { fraction: 0.5 }),
        SettingPress::Drag
    );
    assert_eq!(
        setting_press(&SettingControl::Choice {
            options: vec!["a".into()],
            chosen: 0
        }),
        SettingPress::DropDown
    );
}
