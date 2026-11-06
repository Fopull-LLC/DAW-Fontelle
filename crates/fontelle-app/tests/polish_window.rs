//! The rest of the polish round (Ty, 2026-11-06: *"continue on the
//! remaining reccomendations"*), in the window: the rack glides, the long
//! pages answer Page Up/Down/Home/End, the guide opens where it was left,
//! pages fade in, and a touchpad pans the arrangement under the fingers.

mod common;

use fontelle_ui::canvas::PageKey;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fontelle-polish-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn studio(dir: &std::path::Path, channels: usize) -> fontelle_ui::Windowless {
    let mut project = common::a_clip_project(4);
    for n in 0..channels {
        let mut add = fontelle_model::AddChannel::new(format!("Extra {n}"), None);
        fontelle_model::Command::apply(&mut add, &mut project).expect("adds");
    }
    let session = common::a_session_for(project).with_settings_path(dir.join("settings.json"));
    fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1280, 720),
    )
}

fn centre(r: fontelle_ui::layout::Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

#[test]
fn the_rack_glides_a_notch_rather_than_jumping_it() {
    let dir = scratch("rack");
    let mut w = studio(&dir, 40);
    let (x, y) = centre(w.rack_list());
    w.wheel_at(x, y, -2.0);
    let target = w.rack_scroll_target();
    assert!(target > 0, "the notches moved the list");
    w.animate(1.0 / 60.0);
    let moving = w.rack_drawn_scroll();
    assert!(
        moving > 0.0 && moving < target as f32,
        "{moving} on the way to {target}"
    );
    w.animate(1.0);
    assert_eq!(w.rack_drawn_scroll(), target as f32);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_long_pages_answer_page_keys() {
    let dir = scratch("keys");
    let mut w = studio(&dir, 0);
    w.open_settings_page();
    let longest = (0..w.settings_sections().len())
        .max_by_key(|&i| {
            w.choose_settings_section(i);
            w.settings_page_scroll_max() as i64
        })
        .unwrap();
    w.choose_settings_section(longest);
    let max = w.settings_page_scroll_max();
    assert!(max > 0.0);
    w.page_key(PageKey::End);
    w.animate(1.0 / 60.0);
    let on_the_way = w.settings_page_place().unwrap().1;
    assert!(on_the_way > 0.0 && on_the_way < max, "glides: {on_the_way}");
    w.animate(1.0);
    assert_eq!(w.settings_page_place().unwrap().1, max);
    w.page_key(PageKey::Home);
    w.animate(1.0);
    assert_eq!(w.settings_page_place().unwrap().1, 0.0);
    w.page_key(PageKey::PageDown);
    w.animate(1.0);
    let down = w.settings_page_place().unwrap().1;
    assert!(down > 0.0 && down <= max, "{down}");
    w.page_key(PageKey::PageUp);
    w.animate(1.0);
    assert_eq!(w.settings_page_place().unwrap().1, 0.0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_guide_opens_where_it_was_left() {
    let dir = scratch("guide");
    let mut w = studio(&dir, 0);
    w.open_help_page();
    assert_eq!(w.help_place().unwrap().0, 0);
    w.choose_help_section(2);
    w.wheel_lines(-2.0);
    w.animate(1.0);
    let (section, scroll) = w.help_place().unwrap();
    assert_eq!(section, 2);
    w.close_help_page();
    w.open_help_page();
    let (again, at) = w.help_place().unwrap();
    assert_eq!(again, 2);
    assert!((at - scroll).abs() < 1.0, "{at} vs {scroll}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_page_fades_in_rather_than_blinking_in() {
    let dir = scratch("fade");
    let mut w = studio(&dir, 0);
    w.open_settings_page();
    assert!(w.page_appear() < 0.5, "{}", w.page_appear());
    w.animate(0.3);
    assert_eq!(w.page_appear(), 1.0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_touchpad_pans_the_arrangement_under_the_fingers_and_coasts() {
    let dir = scratch("pan");
    let mut w = studio(&dir, 0);
    let (_, grid, view) = w.timeline_ruler_and_view();
    let (x, y) = centre(grid);
    let ppt = f64::from(view.pixels_per_tick);
    let before = w.timeline_scroll_tick();
    // Fingers move the content left 60 points: the view goes 60 points on.
    w.touchpad_at(x, y, -60.0, 0.0);
    w.animate(0.0);
    let moved = (w.timeline_scroll_tick() - before) as f64 * ppt;
    assert!(
        (moved - 60.0).abs() < 2.0,
        "exactly under the fingers: {moved}"
    );
    // A swipe, and the lift: it carries on.
    for _ in 0..8 {
        w.animate(1.0 / 120.0);
        w.touchpad_at(x, y, -25.0, 0.0);
    }
    let lifted = w.timeline_scroll_tick();
    w.touchpad_lift();
    w.animate(1.5);
    assert!(
        (w.timeline_scroll_tick() - lifted) as f64 * ppt > 100.0,
        "it coasted"
    );
    std::fs::remove_dir_all(&dir).ok();
}
