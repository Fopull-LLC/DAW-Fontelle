//! The seam between the soundfont bank and its presets.
//!
//! Reported from using the window:
//!
//! > *"please also make it so that i can adjust the sizing between the
//! > soundfonts top section and bottom section like often i may have more on
//! > one then the other when searching and i need to focus it more so give it
//! > a knob i can drag in the middle to make whichever one i want bigger
//! > whenever i want."*
//!
//! The split was a constant (`FILE_SHARE`), which is right until the moment
//! one list is forty rows and the other is two. It is a drag now, and the same
//! shape as the sidebar's own seam: a pure function from a pointer position to
//! a share, so "it cannot be dragged until one list has no rows in it" is a
//! test rather than something to find out by doing it.

use fontelle_ui::canvas::{
    BrowserHit, BrowserMode, browser_file_share_at, browser_hit, browser_layout_split,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn body() -> Rect {
    Rect::new(0.0, 0.0, 248.0, 600.0)
}

fn layout(share: Option<f32>) -> fontelle_ui::canvas::BrowserLayout {
    browser_layout_split(
        body(),
        &metrics(),
        BrowserMode::Sounds,
        200,
        200,
        0,
        0,
        share,
    )
}

#[test]
fn the_sounds_tab_has_a_seam_between_its_two_lists() {
    let l = layout(None);
    assert!(!l.seam.is_empty(), "there is nothing to grab");
    // Between them, touching neither's rows.
    assert!(
        l.seam.y >= l.files.bottom() - 1.0,
        "{:?} vs {:?}",
        l.seam,
        l.files
    );
    assert!(
        l.seam.bottom() <= l.presets.y + 1.0,
        "{:?} vs {:?}",
        l.seam,
        l.presets
    );
}

#[test]
fn only_the_tab_with_two_lists_has_one() {
    for mode in [
        BrowserMode::Projects,
        BrowserMode::Import,
        BrowserMode::Settings,
    ] {
        let l = browser_layout_split(body(), &metrics(), mode, 200, 200, 0, 0, None);
        assert!(l.seam.is_empty(), "{mode:?} has one list and a seam");
    }
}

#[test]
fn a_press_on_the_seam_says_so() {
    let l = layout(None);
    let hit = browser_hit(
        &l,
        l.seam.x + l.seam.width / 2.0,
        l.seam.y + l.seam.height / 2.0,
    );
    assert_eq!(hit, BrowserHit::Seam);
}

#[test]
fn dragging_it_down_gives_the_bank_more_and_the_presets_less() {
    let low = layout(Some(0.3));
    let high = layout(Some(0.75));
    assert!(
        high.files.height > low.files.height,
        "{} vs {}",
        high.files.height,
        low.files.height
    );
    assert!(
        high.presets.height < low.presets.height,
        "{} vs {}",
        high.presets.height,
        low.presets.height
    );
}

#[test]
fn neither_list_can_be_dragged_away_to_nothing() {
    // A seam that can be pushed to either end is a list you cannot get back:
    // there is nothing left to grab the seam by.
    for share in [-5.0, 0.0, 0.001, 0.999, 1.0, 40.0] {
        let l = layout(Some(share));
        assert!(l.files.height > 0.0, "share {share}: the bank vanished");
        assert!(
            l.presets.height > 0.0,
            "share {share}: the presets vanished"
        );
        assert!(!l.seam.is_empty(), "share {share}: the seam went with it");
    }
}

#[test]
fn both_lists_stay_a_whole_number_of_rows_tall() {
    // The rule the constant split already kept: a part-row along the bottom
    // edge looks like a row and hit-tests as nothing.
    let m = metrics();
    for share in [0.2, 0.35, 0.5, 0.66, 0.8] {
        let l = layout(Some(share));
        for (name, list) in [("files", l.files), ("presets", l.presets)] {
            let rows = list.height / m.row_height;
            assert!(
                (rows - rows.round()).abs() < 0.01,
                "share {share}: {name} is {rows} rows tall"
            );
        }
    }
}

#[test]
fn the_share_a_drag_asks_for_follows_the_pointer_down_the_panel() {
    let l = layout(None);
    let near_top = browser_file_share_at(&l, l.files.y + 10.0);
    let near_bottom = browser_file_share_at(&l, l.presets.bottom() - 10.0);
    assert!(near_bottom > near_top, "{near_bottom} vs {near_top}");
    for y in [-500.0, 0.0, 300.0, 5000.0] {
        let share = browser_file_share_at(&l, y);
        assert!((0.0..=1.0).contains(&share), "y={y} gave {share}");
    }
}

#[test]
fn a_panel_too_short_for_two_lists_does_not_produce_a_negative_seam() {
    for height in [0.0, 20.0, 60.0, 120.0] {
        let l = browser_layout_split(
            Rect::new(0.0, 0.0, 248.0, height),
            &metrics(),
            BrowserMode::Sounds,
            200,
            200,
            0,
            0,
            Some(0.5),
        );
        for r in [l.files, l.presets, l.seam] {
            assert!(r.width >= 0.0 && r.height >= 0.0, "h={height}: {r:?}");
        }
    }
}

/// > *"we should also make it start out opened on the audio import tab by
/// > default instead of the soundfonts tab"*
///
/// The studio opens on the tab the window starts in, and that is the
/// enum's default: the browser reads it, the host reads it, and one place
/// says which.
#[test]
fn the_browser_starts_on_the_import_tab() {
    assert_eq!(BrowserMode::default(), BrowserMode::Import);
}

// ------------------------------------------------- the tabs are icons ---
//
// > *"this section is also quite crowded we should make use of icons to
// > make it better designed."*
//
// Five tabs across a 248-pixel sidebar were "Sound: Preset: Project Import
// Setting" — every label clipped. Each mode has a glyph now, and so does
// each kind of file the Import tab shows; the word comes back beside the
// glyph when the tab is wide enough to hold both (a widened sidebar), and
// the tooltip says it the rest of the time.

#[test]
fn every_browser_mode_has_a_glyph_of_its_own() {
    let icons: Vec<_> = BrowserMode::ALL.iter().map(|mode| mode.icon()).collect();
    for (i, a) in icons.iter().enumerate() {
        for b in &icons[i + 1..] {
            assert_ne!(a, b, "two tabs share a glyph: {:?}", BrowserMode::ALL);
        }
    }
}

#[test]
fn every_kind_of_import_has_a_glyph_of_its_own() {
    use fontelle_ui::canvas::kind_icon;
    let icons: Vec<_> = fontelle_types::FolderKind::ALL
        .iter()
        .map(|kind| kind_icon(*kind))
        .collect();
    for (i, a) in icons.iter().enumerate() {
        for b in &icons[i + 1..] {
            assert_ne!(a, b, "two kinds share a glyph");
        }
    }
}

#[test]
fn a_tab_shows_its_word_only_when_there_is_room_beside_the_glyph() {
    use fontelle_ui::canvas::{TAB_WORD_MIN_WIDTH, tab_shows_word};
    // The five tabs of a 248-pixel sidebar: glyphs only.
    assert!(!tab_shows_word(44.0));
    // A sidebar dragged wide: the words come back.
    assert!(tab_shows_word(TAB_WORD_MIN_WIDTH));
    assert!(tab_shows_word(140.0));
    assert!(!tab_shows_word(TAB_WORD_MIN_WIDTH - 1.0));
}
