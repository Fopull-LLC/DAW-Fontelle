//! Choosing several rows on the arrangement: Shift-click a range, Ctrl-click
//! one in or out; the row menu's Delete takes them all, as one undo.

mod common;

use fontelle_ui::document::DocumentHost;
use winit::event::MouseButton;

fn centre(r: fontelle_ui::layout::Rect) -> (f32, f32) {
    (r.x + r.width * 0.3, r.y + r.height / 2.0)
}

fn names(w: &fontelle_ui::Windowless) -> Vec<String> {
    w.document().lanes().into_iter().map(|l| l.name).collect()
}

#[test]
fn shift_clicked_rows_delete_together_and_come_back_together() {
    let mut w = fontelle_ui::Windowless::new(
        Box::new(common::a_session_for(fontelle_app::blank_project(
            8, 120.0, 48_000,
        ))),
        fontelle_ui::theme::Theme::dark_default(),
        (1600, 1000),
    );
    let before = names(&w);
    let (x, y) = centre(w.lane_header(1));
    w.click_studio(x, y);
    w.hold(true, false);
    let (x, y) = centre(w.lane_header(3));
    w.click_studio(x, y);
    w.hold(false, false);
    assert_eq!(w.lane_set(), &[1, 2, 3]);

    // Ctrl takes one back out.
    w.hold(false, true);
    let (x, y) = centre(w.lane_header(2));
    w.click_studio(x, y);
    w.hold(false, false);
    assert_eq!(w.lane_set(), &[1, 3]);

    let (x, y) = centre(w.lane_header(3));
    w.press_studio(MouseButton::Right, x, y);
    w.release_studio(MouseButton::Right);
    let delete = w
        .menu_rows()
        .into_iter()
        .find(|(_, label)| label.to_lowercase().contains("delete"))
        .expect("the row menu offers to delete");
    let (dx, dy) = (delete.0.x + 10.0, delete.0.y + delete.0.height / 2.0);
    w.click_studio(dx, dy);
    let after = names(&w);
    assert_eq!(after.len(), before.len() - 2, "{after:?}");
    assert!(!after.contains(&before[1]) && !after.contains(&before[3]));
    assert!(after.contains(&before[2]));
    DocumentHost::undo(w.document_mut());
    assert_eq!(names(&w), before);
}
