//! Lists that glide (Ty, 2026-11-06): the rack and the browser's two lists
//! are laid out at a fraction of a row, so a notch is seen to move them.
//!
//! The rule from the panel's history still holds (`browser::rows`): a row
//! is never squeezed to fit — it keeps its whole height and the list is
//! clipped round it — and a press is answered only by the part of a row
//! that is inside its list, so a click at the top of the presets can never
//! land on a soundfont half-scrolled out of the list above.

use fontelle_ui::canvas::{
    BrowserHit, BrowserMode, RackHit, browser_hit, browser_layout_split, browser_layout_split_at,
    rack_hit, rack_layout, rack_layout_at,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn body() -> Rect {
    Rect::new(0.0, 0.0, 300.0, 400.0)
}

#[test]
fn a_whole_row_lays_out_exactly_as_before() {
    let m = Theme::dark_default().metrics;
    let whole = rack_layout(body(), &m, 50, 4);
    let at = rack_layout_at(body(), &m, 50, 4.0);
    assert_eq!(whole.rows.len(), at.rows.len());
    for (a, b) in whole.rows.iter().zip(&at.rows) {
        assert_eq!((a.index, a.frame), (b.index, b.frame));
    }
    let b1 = browser_layout_split(body(), &m, BrowserMode::Sounds, 40, 40, 3, 5, None);
    let b2 = browser_layout_split_at(body(), &m, BrowserMode::Sounds, 40, 40, 3.0, 5.0, None);
    assert_eq!(b1.file_rows, b2.file_rows);
    assert_eq!(b1.preset_rows, b2.preset_rows);
}

#[test]
fn half_a_row_down_the_rack_shows_half_of_the_first_and_one_more() {
    let m = Theme::dark_default().metrics;
    let whole = rack_layout(body(), &m, 50, 4);
    let half = rack_layout_at(body(), &m, 50, 4.5);
    let row = m.row_height;
    assert_eq!(half.rows[0].index, 4);
    assert!((half.rows[0].frame.y - (half.list.y - row / 2.0)).abs() < 0.01);
    assert_eq!(
        half.rows[0].frame.height, row,
        "whole height, clipped by the list"
    );
    assert_eq!(
        half.rows.len(),
        whole.rows.len() + 1,
        "one more peeks in below"
    );
    let last = half.rows.last().unwrap();
    assert!(last.frame.bottom() >= half.list.bottom() - 0.01);
    // Below the list, on the bit of the last row that hangs out of it:
    // nothing — never the row.
    let x = half.list.x + 20.0;
    let y = half.list.bottom() + 1.0;
    if last.frame.contains(x, y) {
        assert!(!matches!(rack_hit(&half, x, y), RackHit::Row(_)));
    }
}

#[test]
fn half_a_row_down_the_browser_never_answers_for_the_list_beside_it() {
    let m = Theme::dark_default().metrics;
    let l = browser_layout_split_at(body(), &m, BrowserMode::Sounds, 40, 40, 6.5, 2.5, None);
    let (first, rect) = l.file_rows[0];
    assert_eq!(first, 6);
    assert!(rect.y < l.files.y, "the first file is half out the top");
    let (_, preset) = l.preset_rows[0];
    assert!(preset.y < l.presets.y);
    // The top few points of the presets list: a preset, never a file.
    let x = l.presets.x + 10.0;
    let y = l.presets.y + 1.0;
    assert!(matches!(browser_hit(&l, x, y), BrowserHit::Preset(_)));
    // The bottom few points of the files list: a file, never a preset.
    let y = l.files.bottom() - 1.0;
    assert!(
        matches!(
            browser_hit(&l, x, y),
            BrowserHit::File(_) | BrowserHit::Nothing
        ),
        "{:?}",
        browser_hit(&l, x, y)
    );
}
