//! A preference, off by default: a note sounds when it is placed, and when
//! a drag moves it to another key — never on every pixel of the drag.
//! Placing and dragging are silent by default on purpose (an earlier report
//! asked for exactly that), so nobody's habit changes unless they ask.

mod common;

fn a_window(on: bool) -> fontelle_ui::Windowless {
    let dir =
        std::env::temp_dir().join(format!("fontelle-placed-sound-{on}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut session = common::a_session_for(common::a_clip_project(8))
        .with_settings_path(dir.join("settings.json"));
    if on {
        session.settings_mut().sound_placed_notes = true;
    }
    fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1600, 1000),
    )
}

#[test]
fn off_by_default_a_placed_note_is_silent() {
    let mut w = a_window(false);
    assert!(!w.document().sounds_placed_notes());
    let (x, y) = w.roll_point(0, 64);
    w.click_studio(x, y);
    assert_eq!(w.auditioning(), None);
}

#[test]
fn on_a_placed_note_sounds_and_a_drag_to_another_key_sounds_the_new_key() {
    let mut w = a_window(true);
    let (x, y) = w.roll_point(0, 64);
    w.click_studio(x, y);
    assert_eq!(w.auditioning(), Some(64), "the placed note sounds");
    // Grab it and drag it up a key.
    let (x, y) = w.roll_point(PPQ / 8, 64);
    w.press_studio(winit::event::MouseButton::Left, x, y);
    let (x2, y2) = w.roll_point(PPQ / 8, 65);
    w.move_studio(x2, y2);
    assert_eq!(w.auditioning(), Some(65), "the new key sounds");
    w.release_studio(winit::event::MouseButton::Left);
}

const PPQ: i64 = fontelle_types::PPQN;

/// A deleted note bursts, and the flecks are gone in a moment.
#[test]
fn a_deleted_note_bursts_and_is_gone() {
    let mut w = a_window(false);
    let (x, y) = w.roll_point(0, 64);
    w.click_studio(x, y);
    // The right button erases.
    w.press_studio(winit::event::MouseButton::Right, x, y);
    w.release_studio(winit::event::MouseButton::Right);
    assert!(w.flecks() > 0, "no burst");
    for _ in 0..30 {
        w.frame(1.0 / 60.0);
    }
    assert_eq!(w.flecks(), 0, "still in the air");
}
