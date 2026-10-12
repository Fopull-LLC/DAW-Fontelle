//! A window drawn on a scaled display fills it, and the pointer lands on
//! what it is over.
//!
//! Ty, on a Windows laptop: *"it does not size the window correctly and my
//! mouse input it's detecting is way offset to my actual cursor. it's like
//! it's squishing everything into the top corner and squishing my mouse
//! offset in that direction too."*
//!
//! **The root cause.** The window lays itself out in *logical* points
//! (`App::resize` divides the surface's physical size by the scale factor)
//! and turns the pointer into logical points (`CursorMoved` divides by it
//! too) — both right. But the scene built from that layout was handed to
//! the renderer as it was, and the renderer draws one scene unit to one
//! **physical** pixel. At 100 % the two are the same, which is every display
//! this was ever looked at on. At 150 % — a laptop's default on Windows —
//! a window 1920 pixels wide is laid out 1280 points wide and drawn 1280
//! pixels wide: everything squashed into the top-left two thirds, and every
//! click landing on what is drawn 1.5 times further from the corner than the
//! pointer. Not a manifest: winit makes the process per-monitor DPI aware
//! itself, which is why the scale factor was there to divide by at all.
//!
//! The fix is one transform at the one place a frame meets a surface:
//! [`for_surface`] scales the logical scene up to the surface's pixels.

use fontelle_ui::render::{Headless, for_surface};
use vello::Scene;
use vello::kurbo::{Affine, Rect};
use vello::peniko::{Color, Fill};

const RED: Color = Color::from_rgba8(0xe0, 0x20, 0x20, 0xff);

/// A 40 × 40 point square at (20, 20), drawn in points.
fn logical_scene() -> Scene {
    let mut scene = Scene::new();
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        RED,
        None,
        &Rect::new(20.0, 20.0, 60.0, 60.0),
    );
    scene
}

#[test]
fn at_one_to_one_the_scene_is_handed_on_as_it_is() {
    let scene = logical_scene();
    let mut frame = Scene::new();
    let out = for_surface(&scene, 1.0, &mut frame);
    assert!(std::ptr::eq(out, &scene), "no copy at 100 %");
}

#[test]
fn a_nonsense_scale_is_treated_as_one() {
    let scene = logical_scene();
    for scale in [0.0, -2.0, f64::NAN, f64::INFINITY] {
        let mut frame = Scene::new();
        assert!(std::ptr::eq(for_surface(&scene, scale, &mut frame), &scene));
    }
}

#[test]
fn at_150_percent_a_square_drawn_in_points_covers_its_pixels() {
    // The Windows runner's software adapter dies inside the driver — see
    // `render_headless.rs`'s `headless()`.
    if cfg!(windows) && std::env::var_os("CI").is_some() {
        return;
    }
    let Ok(mut headless) = Headless::new() else {
        eprintln!("skipping: no usable GPU adapter");
        return;
    };
    let scene = logical_scene();
    let mut frame = Scene::new();
    let out = for_surface(&scene, 1.5, &mut frame);
    let (w, h) = (120u32, 120u32);
    let pixels = headless
        .render(out, w, h, fontelle_ui::theme::Color::rgb(0x10, 0x10, 0x10))
        .expect("renders");
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    // The square's points 20..60 are pixels 30..90.
    assert_eq!(
        at(85, 85),
        [0xe0, 0x20, 0x20],
        "the far corner reaches pixel 90"
    );
    assert_eq!(at(33, 33), [0xe0, 0x20, 0x20]);
    assert_ne!(
        at(25, 25),
        [0xe0, 0x20, 0x20],
        "and starts at pixel 30, not 20"
    );
    assert_ne!(at(95, 95), [0xe0, 0x20, 0x20]);
}
