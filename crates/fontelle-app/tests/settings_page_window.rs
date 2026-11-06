//! The settings page, as a person uses it (Ty, 2026-11-06: *"make settings
//! remember what tab you were last in so every time you dont have to re
//! scroll and stuff"*), and its springy scroll (*"springy scrolling like
//! apple devices have"*).

mod common;

fn studio_in(dir: &std::path::Path) -> fontelle_ui::Windowless {
    let session = common::a_session_for(common::a_clip_project(4))
        .with_settings_path(dir.join("settings.json"));
    fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1280, 720),
    )
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fontelle-settings-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The section a page shows, and the longest section's place in the list —
/// the one worth scrolling.
fn longest_section(w: &mut fontelle_ui::Windowless) -> usize {
    let n = w.settings_sections().len();
    assert!(n >= 3, "a page with sections: {n}");
    (0..n)
        .max_by_key(|&i| {
            w.choose_settings_section(i);
            w.settings_page_scroll_max() as i64
        })
        .unwrap()
}

#[test]
fn settings_opens_where_it_was_left() {
    let dir = scratch("remember");
    let mut w = studio_in(&dir);
    w.open_settings_page();
    assert_eq!(
        w.settings_page_place().unwrap().0,
        0,
        "first time: the first section"
    );
    let longest = longest_section(&mut w);
    assert!(longest > 0, "pick a section that is not the first");
    w.choose_settings_section(longest);
    let max = w.settings_page_scroll_max();
    assert!(max > 30.0, "and that scrolls: {max}");
    w.wheel_lines(-3.0);
    w.animate(1.0);
    let (section, scroll) = w.settings_page_place().unwrap();
    assert_eq!(section, longest);
    assert!(scroll > 30.0, "scrolled down: {scroll}");
    w.close_settings_page();
    assert!(!w.settings_page_open());

    // The gear again: the same section, the same place on it.
    w.open_settings_page();
    let (again, at) = w.settings_page_place().unwrap();
    assert_eq!(again, longest);
    assert!((at - scroll).abs() < 1.0, "{at} vs {scroll}");
    w.close_settings_page();
    drop(w);

    // And the next time Fontelle opens.
    let mut w = studio_in(&dir);
    w.open_settings_page();
    let (next_launch, at) = w.settings_page_place().unwrap();
    assert_eq!(next_launch, longest);
    assert!((at - scroll).abs() < 1.0, "{at} vs {scroll}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn asked_for_by_name_it_opens_there_and_remembers_that() {
    // "Import from", from the browser's own button: the named section, at
    // its top — and that is where the gear opens next.
    let dir = scratch("named");
    let mut w = studio_in(&dir);
    w.open_settings_page();
    let longest = longest_section(&mut w);
    w.choose_settings_section(longest);
    w.close_settings_page();
    w.open_settings("Import from");
    let (named, scroll) = w.settings_page_place().unwrap();
    assert_ne!(named, longest);
    assert_eq!(scroll, 0.0);
    w.close_settings_page();
    w.open_settings_page();
    assert_eq!(w.settings_page_place().unwrap().0, named);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_new_section_starts_at_its_top() {
    let dir = scratch("section-top");
    let mut w = studio_in(&dir);
    w.open_settings_page();
    let longest = longest_section(&mut w);
    w.choose_settings_section(longest);
    w.wheel_lines(-3.0);
    w.animate(1.0);
    let other = if longest == 0 { 1 } else { 0 };
    w.choose_settings_section(other);
    assert_eq!(w.settings_page_place().unwrap(), (other, 0.0));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_page_glides_and_springs_back_at_its_top() {
    let dir = scratch("spring");
    let mut w = studio_in(&dir);
    w.open_settings_page();
    let longest = longest_section(&mut w);
    w.choose_settings_section(longest);
    // A notch down glides there rather than jumping.
    w.wheel_lines(-1.0);
    w.animate(1.0 / 60.0);
    let first = w.settings_page_place().unwrap().1;
    w.animate(1.0);
    let landed = w.settings_page_place().unwrap().1;
    assert!(first > 0.0 && first < landed, "{first} then {landed}");
    // Up past the top: it gives, then comes back to exactly the top.
    w.wheel_lines(4.0);
    let mut deepest = f32::MAX;
    for _ in 0..90 {
        w.animate(1.0 / 60.0);
        deepest = deepest.min(w.settings_page_place().unwrap().1);
    }
    assert!(deepest < -1.0, "it gave: {deepest}");
    assert_eq!(w.settings_page_place().unwrap().1, 0.0);
    // A touchpad follows the fingers, and coasts when they lift.
    w.touchpad(-60.0);
    assert_eq!(w.settings_page_place().unwrap().1, 60.0);
    for _ in 0..6 {
        w.animate(1.0 / 120.0);
        w.touchpad(-20.0);
    }
    let lifted = w.settings_page_place().unwrap().1;
    w.touchpad_lift();
    w.animate(1.5);
    assert!(
        w.settings_page_place().unwrap().1 > lifted + 20.0,
        "it coasted"
    );
    std::fs::remove_dir_all(&dir).ok();
}
