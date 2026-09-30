//! The settings page: a full page over the studio, opened by the gear.
//!
//! Ty, 2026-09-30 (`docs/ux-routing-and-learning-plan.md` §6): *"a full
//! settings page"* — a section list on the left and roomy, clearly styled
//! controls on the right; Escape closes it. The report it answers: *"formatted
//! weird like everything looks like a button even when things are just
//! labels, some things just have no or little feedback"*.
//!
//! The page is laid out from the same flat list of rows the host has always
//! given (`StudioHost::settings` and `setting_controls`): a heading row starts
//! a section, so the host adds a section by adding a heading and the window
//! learns nothing about what any row means.

use fontelle_ui::canvas::{
    SettingControl, SettingsPageHit, SettingsPageLayout, SettingsSection, settings_page_hit,
    settings_page_layout, settings_page_scroll_max, settings_page_scrolled, settings_sections,
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
        && inner.right() <= outer.right() + 0.01
        && inner.bottom() <= outer.bottom() + 0.01
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.right() - 0.01
        && b.x < a.right() - 0.01
        && a.y < b.bottom() - 0.01
        && b.y < a.bottom() - 0.01
}

fn button(caption: &str) -> SettingControl {
    SettingControl::Button {
        caption: caption.to_string(),
    }
}

/// The shape the host gives: MIDI input (three rows), Import from (two),
/// Extensions (none yet), Updates (one).
fn controls() -> Vec<SettingControl> {
    vec![
        SettingControl::Heading,
        SettingControl::Choice {
            options: vec!["Linear".into(), "Soft".into()],
            chosen: 0,
        },
        SettingControl::Slider { fraction: 0.5 },
        SettingControl::Slider { fraction: 0.2 },
        SettingControl::Heading,
        button("Choose\u{2026}"),
        button("Choose\u{2026}"),
        SettingControl::Heading,
        SettingControl::Heading,
        SettingControl::Switch { on: true },
    ]
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn layout_at(section: usize, scroll: f32) -> (Vec<SettingsSection>, SettingsPageLayout) {
    let sections = settings_sections(&controls());
    let layout = settings_page_layout(window(), &metrics(), &sections, section, scroll);
    (sections, layout)
}

// ------------------------------------------------------------ the sections

#[test]
fn a_heading_row_starts_a_section_and_owns_the_rows_under_it() {
    let sections = settings_sections(&controls());
    assert_eq!(
        sections,
        vec![
            SettingsSection {
                heading: 0,
                rows: 1..4
            },
            SettingsSection {
                heading: 4,
                rows: 5..7
            },
            // Listed even with nothing in it yet: a section that vanishes
            // while empty is a section nobody learns is there.
            SettingsSection {
                heading: 7,
                rows: 8..8
            },
            SettingsSection {
                heading: 8,
                rows: 9..10
            },
        ]
    );
}

#[test]
fn rows_before_any_heading_are_a_section_of_their_own_rather_than_lost() {
    let sections = settings_sections(&[
        SettingControl::Switch { on: false },
        SettingControl::Heading,
    ]);
    assert_eq!(sections[0].rows, 0..1);
    assert_eq!(sections.len(), 2);
}

// -------------------------------------------------------------- the layout

#[test]
fn the_page_has_a_section_list_on_the_left_and_the_rows_on_the_right() {
    let (sections, l) = layout_at(0, 0.0);
    assert!(within(l.frame, window()));
    assert_eq!(l.nav.len(), sections.len(), "one entry per section");
    for (a, b) in l.nav.iter().zip(l.nav.iter().skip(1)) {
        assert!(
            b.y >= a.bottom() - 0.01,
            "stacked top to bottom: {a:?} {b:?}"
        );
    }
    for entry in &l.nav {
        assert!(within(*entry, l.frame));
        assert!(entry.right() <= l.body.x, "the list is left of the body");
    }
    assert!(within(l.body, l.frame));
    assert!(within(l.close, l.frame));
    assert!(!l.close.is_empty());
    assert!(l.close.x > l.body.x, "the \u{d7} is at the top right");
}

#[test]
fn only_the_chosen_sections_rows_are_laid_out_in_order() {
    let (_, l) = layout_at(0, 0.0);
    let indices: Vec<usize> = l.rows.iter().map(|r| r.index).collect();
    assert_eq!(indices, vec![1, 2, 3]);
    let (_, l) = layout_at(1, 0.0);
    let indices: Vec<usize> = l.rows.iter().map(|r| r.index).collect();
    assert_eq!(indices, vec![5, 6]);
    let (_, l) = layout_at(2, 0.0);
    assert!(l.rows.is_empty(), "an empty section lays out nothing");
}

#[test]
fn a_row_is_roomy_with_its_words_on_the_left_and_its_control_on_the_right() {
    let (_, l) = layout_at(0, 0.0);
    let m = metrics();
    for row in &l.rows {
        assert!(
            row.rect.height >= m.row_height * 2.0,
            "roomy, with room for a help line: {row:?}"
        );
        assert!(within(row.label, row.rect));
        assert!(within(row.help, row.rect));
        assert!(within(row.control, row.rect));
        assert!(
            row.help.y >= row.label.bottom() - 0.01,
            "help under the name"
        );
        assert!(!overlaps(row.label, row.control), "{row:?}");
        assert!(!overlaps(row.help, row.control), "{row:?}");
        assert!(row.control.x > row.label.x);
        assert!(
            row.control.height < row.rect.height,
            "a control sized as a control, not the whole row"
        );
    }
    for (a, b) in l.rows.iter().zip(l.rows.iter().skip(1)) {
        assert!(b.rect.y >= a.rect.bottom() - 0.01);
    }
}

#[test]
fn a_narrow_window_still_keeps_everything_on_the_card_and_apart() {
    let small = Rect::new(0.0, 0.0, 640.0, 400.0);
    let sections = settings_sections(&controls());
    let l = settings_page_layout(small, &metrics(), &sections, 0, 0.0);
    assert!(within(l.frame, small));
    for row in &l.rows {
        assert!(within(row.control, l.frame));
        assert!(!overlaps(row.label, row.control));
    }
    for entry in &l.nav {
        assert!(entry.right() <= l.body.x);
    }
}

#[test]
fn a_section_past_the_end_falls_back_to_the_first() {
    let (_, l) = layout_at(99, 0.0);
    assert_eq!(l.section, 0);
    assert_eq!(l.rows.first().map(|r| r.index), Some(1));
}

// ------------------------------------------------------------------ scroll

#[test]
fn a_section_taller_than_the_body_scrolls_and_the_scroll_is_clamped() {
    let mut many = vec![SettingControl::Heading];
    many.extend((0..40).map(|_| SettingControl::Switch { on: false }));
    let sections = settings_sections(&many);
    let l = settings_page_layout(window(), &metrics(), &sections, 0, 0.0);
    let max = settings_page_scroll_max(&l);
    assert!(max > 0.0);
    let down = settings_page_scrolled(&l, 0.0, -3.0);
    assert!(down > 0.0, "a notch down scrolls down");
    assert_eq!(settings_page_scrolled(&l, 0.0, 3.0), 0.0);
    let l = settings_page_layout(window(), &metrics(), &sections, 0, 1.0e6);
    assert_eq!(l.scroll, max);
    for row in &l.rows {
        assert!(row.rect.intersects(&l.body), "only rows that show: {row:?}");
    }
}

// --------------------------------------------------------------------- hit

#[test]
fn a_press_is_named_by_what_it_lands_on() {
    let (_, l) = layout_at(0, 0.0);
    let (x, y) = centre(l.close);
    assert_eq!(settings_page_hit(&l, x, y), SettingsPageHit::Close);
    let (x, y) = centre(l.nav[3]);
    assert_eq!(settings_page_hit(&l, x, y), SettingsPageHit::Section(3));
    let row = l.rows[1];
    let (x, y) = centre(row.control);
    assert_eq!(
        settings_page_hit(&l, x, y),
        SettingsPageHit::Row {
            index: 2,
            on_control: true
        }
    );
    let (x, y) = centre(row.label);
    assert_eq!(
        settings_page_hit(&l, x, y),
        SettingsPageHit::Row {
            index: 2,
            on_control: false
        }
    );
    assert_eq!(settings_page_hit(&l, 2.0, 2.0), SettingsPageHit::Outside);
    let (x, _) = centre(l.body);
    assert_eq!(
        settings_page_hit(&l, x, l.body.bottom() - 2.0),
        SettingsPageHit::Card
    );
}
