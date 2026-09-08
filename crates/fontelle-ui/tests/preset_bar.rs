//! The preset bar: the one widget every editor window carries
//! (`docs/flopsynth-plan.md` §P.7).
//!
//! ```text
//! │ ◀ ▶ │ Choir Ahh*  ▾ │ Choir & Vocal │ ★ │ Save │ Save as… │
//! ```
//!
//! Pure geometry, like every other canvas module: the host works out *what* it
//! says (`Session::preset_state`) and this works out *where*. The two claims
//! worth holding in a test are the ones a person would notice being wrong —
//! that a control which is not drawn cannot be pressed, and that the name says
//! `*` exactly when the device has drifted from its file.

use fontelle_types::PresetOrigin;
use fontelle_ui::canvas::{
    PresetBarHit, PresetBarView, PresetChoice, PresetMenuRow, preset_bar_hit, preset_bar_layout,
    preset_bar_name, preset_menu,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn a_view() -> PresetBarView {
    PresetBarView {
        name: Some("Choir Ahh".to_string()),
        category: "Choir & Vocal".to_string(),
        origin: Some(PresetOrigin::Factory),
        dirty: false,
        favourite: false,
        can_save: false,
    }
}

fn a_header(width: f32) -> Rect {
    Rect::new(
        0.0,
        0.0,
        width,
        Theme::dark_default().metrics.panel_header_height,
    )
}

/// The widths the bar has to work at: Flopsynth's window, the knob grid's, and
/// the narrowest an editor window is allowed to get.
const WIDTHS: [f32; 3] = [1180.0, 620.0, 360.0];

// ------------------------------------------------------------------ layout

#[test]
fn every_control_sits_inside_the_header_at_every_width() {
    let theme = Theme::dark_default();
    for width in WIDTHS {
        let header = a_header(width);
        let layout = preset_bar_layout(header, 120.0, &a_view(), &theme.metrics);
        for (name, rect) in layout.controls() {
            if rect.is_empty() {
                continue;
            }
            assert!(
                rect.x >= header.x - 0.01
                    && rect.right() <= header.right() + 0.01
                    && rect.y >= header.y - 0.01
                    && rect.bottom() <= header.bottom() + 0.01,
                "{name} at {width}: {rect:?} is outside {header:?}"
            );
        }
    }
}

#[test]
fn no_two_controls_overlap_at_any_width() {
    let theme = Theme::dark_default();
    for width in WIDTHS {
        let layout = preset_bar_layout(a_header(width), 120.0, &a_view(), &theme.metrics);
        let drawn: Vec<_> = layout
            .controls()
            .into_iter()
            .filter(|(_, rect)| !rect.is_empty())
            .collect();
        for (i, (one, a)) in drawn.iter().enumerate() {
            for (two, b) in &drawn[i + 1..] {
                assert!(!a.intersects(b), "{one} and {two} overlap at {width}");
            }
        }
    }
}

#[test]
fn the_name_is_the_last_thing_to_go_when_there_is_no_room() {
    // A bar with no room for its buttons is still worth having, because the
    // name is what a person reads to know what they are hearing. The buttons
    // are gestures with other routes to them; the name has none.
    let theme = Theme::dark_default();
    let layout = preset_bar_layout(a_header(300.0), 120.0, &a_view(), &theme.metrics);
    assert!(!layout.name.is_empty(), "the name went before the buttons");
}

#[test]
fn a_control_with_no_room_is_empty_rather_than_squashed() {
    let theme = Theme::dark_default();
    let wide = preset_bar_layout(a_header(1180.0), 120.0, &a_view(), &theme.metrics);
    let narrow = preset_bar_layout(a_header(300.0), 120.0, &a_view(), &theme.metrics);
    assert!(!wide.category.is_empty());
    assert!(
        narrow.category.is_empty(),
        "the category should drop out rather than shrink to nothing"
    );
}

#[test]
fn the_bar_keeps_clear_of_the_title() {
    // The header's left end carries the window's own name. The bar is told how
    // much of it that took rather than measuring text, because a canvas has no
    // font — the same arrangement every other pure layout function here has.
    let theme = Theme::dark_default();
    let layout = preset_bar_layout(a_header(620.0), 300.0, &a_view(), &theme.metrics);
    assert!(
        layout.frame.x >= 300.0,
        "the bar started at {} and the title needs 300",
        layout.frame.x
    );
}

// -------------------------------------------------------------------- hits

#[test]
fn every_control_is_hit_at_its_own_centre() {
    let theme = Theme::dark_default();
    let mut view = a_view();
    // A user preset with unsaved edits: the one state in which every control
    // is live at once.
    view.origin = Some(PresetOrigin::User);
    view.dirty = true;
    view.can_save = true;
    let layout = preset_bar_layout(a_header(1180.0), 120.0, &view, &theme.metrics);
    for (expected, rect) in [
        (PresetBarHit::Previous, layout.previous),
        (PresetBarHit::Next, layout.next),
        (PresetBarHit::Name, layout.name),
        (PresetBarHit::Category, layout.category),
        (PresetBarHit::Star, layout.star),
        (PresetBarHit::Save, layout.save),
        (PresetBarHit::SaveAs, layout.save_as),
    ] {
        assert!(!rect.is_empty(), "{expected:?} was not laid out");
        let hit = preset_bar_hit(
            &layout,
            &view,
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
        );
        assert_eq!(hit, Some(expected), "at {rect:?}");
    }
}

#[test]
fn a_disabled_save_is_not_hit() {
    // A factory preset is read-only, so Save is drawn and does nothing. It is
    // *drawn* because hiding it would teach nobody why it is not there — the
    // menu's `MenuEntry::disabled` reasoning, applied to a button.
    let theme = Theme::dark_default();
    let view = a_view();
    assert!(!view.can_save);
    let layout = preset_bar_layout(a_header(1180.0), 120.0, &view, &theme.metrics);
    assert!(!layout.save.is_empty(), "a disabled Save is still drawn");
    assert_eq!(
        preset_bar_hit(
            &layout,
            &view,
            layout.save.x + layout.save.width / 2.0,
            layout.save.y + layout.save.height / 2.0
        ),
        None
    );
}

#[test]
fn save_as_works_on_a_factory_preset() {
    // The other half of the same rule: read-only is about the *file*, and
    // "Save as…" writes a different one.
    let theme = Theme::dark_default();
    let view = a_view();
    let layout = preset_bar_layout(a_header(1180.0), 120.0, &view, &theme.metrics);
    assert_eq!(
        preset_bar_hit(
            &layout,
            &view,
            layout.save_as.x + layout.save_as.width / 2.0,
            layout.save_as.y + layout.save_as.height / 2.0
        ),
        Some(PresetBarHit::SaveAs)
    );
}

#[test]
fn a_press_outside_the_bar_hits_nothing() {
    let theme = Theme::dark_default();
    let view = a_view();
    let layout = preset_bar_layout(a_header(1180.0), 120.0, &view, &theme.metrics);
    assert_eq!(preset_bar_hit(&layout, &view, 10.0, 10.0), None);
}

// -------------------------------------------------------------- what it says

#[test]
fn a_device_with_no_preset_says_so_rather_than_showing_a_blank() {
    let mut view = a_view();
    view.name = None;
    view.category = String::new();
    view.origin = None;
    assert_eq!(preset_bar_name(&view), "\u{2014} no preset \u{2014}");
}

#[test]
fn an_edited_preset_wears_a_star() {
    let mut view = a_view();
    assert_eq!(preset_bar_name(&view), "Choir Ahh");
    view.dirty = true;
    assert_eq!(preset_bar_name(&view), "Choir Ahh*");
}

#[test]
fn an_unnamed_device_that_has_been_edited_says_so_too() {
    // §P.6's third case: no ref at all *and* the state is not the device's own
    // init. The host decides that; the bar draws it.
    let mut view = a_view();
    view.name = None;
    view.dirty = true;
    assert_eq!(preset_bar_name(&view), "\u{2014} no preset \u{2014}*");
}

// ------------------------------------------------------------- the drop-down

fn a_choice(name: &str, category: &str, origin: PresetOrigin) -> PresetChoice {
    PresetChoice {
        name: name.to_string(),
        category: category.to_string(),
        origin,
        favourite: false,
    }
}

#[test]
fn the_menu_groups_presets_under_their_category() {
    let choices = vec![
        a_choice("Glass", "Pad", PresetOrigin::Factory),
        a_choice("Warm", "Pad", PresetOrigin::Factory),
        a_choice("Sub", "Bass", PresetOrigin::Factory),
    ];
    let (entries, rows) = preset_menu(&choices, "");
    let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
    // The first row is the search hint; the list is under it.
    assert_eq!(&labels[1..], ["Pad", "Glass", "Warm", "Bass", "Sub"]);
    assert_eq!(
        rows,
        [
            PresetMenuRow::Heading,
            PresetMenuRow::Heading,
            PresetMenuRow::Preset(0),
            PresetMenuRow::Preset(1),
            PresetMenuRow::Heading,
            PresetMenuRow::Preset(2),
        ]
    );
    assert!(!entries[1].enabled, "a heading is not a row you can press");
}

#[test]
fn favourites_come_first_when_there_are_any() {
    // The rule every menu in this program follows (`favorites.rs`): if
    // anything in the list is starred, a Favorites section goes first and
    // holds every one of them — and a favourite is lit in the full list too.
    let mut choices = vec![
        a_choice("Glass", "Pad", PresetOrigin::Factory),
        a_choice("Sub", "Bass", PresetOrigin::Factory),
    ];
    choices[1].favourite = true;
    let (entries, rows) = preset_menu(&choices, "");
    assert_eq!(entries[1].label, "Favorites");
    assert_eq!(entries[2].label, "Sub");
    assert_eq!(rows[2], PresetMenuRow::Preset(1));
    assert!(
        entries
            .iter()
            .filter(|e| e.label == "Sub")
            .all(|e| e.is_favorite()),
        "a favourite is lit wherever it appears"
    );
}

#[test]
fn a_bank_with_nothing_in_it_is_one_greyed_row() {
    let (entries, rows) = preset_menu(&[], "");
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].enabled);
    assert_eq!(rows, [PresetMenuRow::Heading]);
}

#[test]
fn typing_into_the_drop_down_narrows_it_to_matching_presets() {
    // The plugin picker's rule, for the same reason: a hundred and
    // twenty-eight presets is a list to be searched, not only scrolled. The
    // headings of categories with nothing left in them go too, and the first
    // row says what has been typed so the narrowing is visible.
    let mut choices = vec![
        a_choice("Glass", "Pad", PresetOrigin::Factory),
        a_choice("Warm", "Pad", PresetOrigin::Factory),
        a_choice("Sub", "Bass", PresetOrigin::Factory),
    ];
    choices[2].favourite = true;
    let (entries, rows) = preset_menu(&choices, "gl");
    let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
    assert!(
        labels[0].contains("gl"),
        "the first row shows what was typed: {labels:?}"
    );
    assert!(!entries[0].enabled);
    assert_eq!(&labels[1..], ["Pad", "Glass"]);
    assert_eq!(rows[2], PresetMenuRow::Preset(0));
    assert!(
        !labels.contains(&"Bass") && !labels.contains(&"Favorites"),
        "a section with nothing left in it is not shown: {labels:?}"
    );

    // Nothing typed is the whole list, with a hint on the first row.
    let (entries, _) = preset_menu(&choices, "");
    assert!(entries[0].label.contains("type to filter"));
    assert_eq!(entries[1].label, "Favorites");

    // Nothing matching says so rather than showing an empty menu.
    let (entries, rows) = preset_menu(&choices, "zzz");
    assert_eq!(entries.len(), 2);
    assert!(!entries[1].enabled);
    assert_eq!(rows[1], PresetMenuRow::Heading);
}
