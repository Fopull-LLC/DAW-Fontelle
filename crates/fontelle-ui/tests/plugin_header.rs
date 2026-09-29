//! The strip across the top of a hosted plugin's own editor window.
//!
//! > *"we need to ensure our presets system works with it kind of like how
//! > flx does so that you can use the presets system for all plugins
//! > cleanly and it just works."*
//!
//! FL Studio wraps every plugin's editor in a bar of its own with the preset
//! controls on it. A plugin's editor here is a window the plugin draws into,
//! and until this the studio's window with the preset bar on it was closed
//! when that one opened — so a plugin that had its own editor had no preset
//! bar anywhere. The strip is the same bar, drawn by the same code, into the
//! top of the plugin's window; the plugin gets the area below it.

use std::sync::{Mutex, OnceLock};

use fontelle_types::PresetOrigin;
use fontelle_ui::canvas::{
    PLUGIN_HEADER_HEIGHT, PresetBarHit, PresetBarView, plugin_header_hit, plugin_header_layout,
};
use fontelle_ui::render::Headless;
use fontelle_ui::theme::Theme;

fn a_view() -> PresetBarView {
    PresetBarView {
        name: Some("Pulled PWM".to_string()),
        category: "Keys".to_string(),
        origin: Some(PresetOrigin::Plugin),
        dirty: false,
        favourite: false,
        can_save: false,
    }
}

/// Surge XT's editor, OB-Xf's, and the narrowest a plugin's window is
/// likely to be.
const WIDTHS: [f32; 3] = [904.0, 640.0, 280.0];

#[test]
fn the_whole_bar_fits_across_a_plugins_editor() {
    let metrics = Theme::dark_default().metrics;
    let layout = plugin_header_layout(640.0, &a_view(), &metrics);
    for (name, rect) in layout.controls() {
        assert!(!rect.is_empty(), "{name} dropped at 640");
        assert!(
            rect.x >= 0.0 && rect.right() <= 640.0,
            "{name} at {rect:?} is outside the window"
        );
        assert!(
            rect.y >= 0.0 && rect.bottom() <= PLUGIN_HEADER_HEIGHT,
            "{name} at {rect:?} is outside the strip"
        );
    }
}

#[test]
fn a_narrow_plugin_window_still_says_which_preset_it_is_on() {
    let metrics = Theme::dark_default().metrics;
    for width in WIDTHS {
        let layout = plugin_header_layout(width, &a_view(), &metrics);
        assert!(!layout.name.is_empty(), "no name at {width}");
    }
}

#[test]
fn a_press_lands_on_the_control_under_it() {
    let metrics = Theme::dark_default().metrics;
    let view = a_view();
    let layout = plugin_header_layout(640.0, &view, &metrics);
    let middle =
        |rect: fontelle_ui::layout::Rect| (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    for (rect, hit) in [
        (layout.previous, PresetBarHit::Previous),
        (layout.next, PresetBarHit::Next),
        (layout.name, PresetBarHit::Name),
        (layout.save_as, PresetBarHit::SaveAs),
    ] {
        let (x, y) = middle(rect);
        assert_eq!(plugin_header_hit(640.0, &view, &metrics, x, y), Some(hit));
    }
    // Save, on the plugin's own read-only preset, is nothing.
    let (x, y) = middle(layout.save);
    assert_eq!(plugin_header_hit(640.0, &view, &metrics, x, y), None);
}

fn headless() -> Option<&'static Mutex<Headless>> {
    if cfg!(windows) && std::env::var_os("CI").is_some() {
        eprintln!("skipping: no GPU on this runner");
        return None;
    }
    static SHARED: OnceLock<Option<Mutex<Headless>>> = OnceLock::new();
    SHARED
        .get_or_init(|| match Headless::new() {
            Ok(h) => Some(Mutex::new(h)),
            Err(e) => {
                eprintln!("skipping: no usable GPU adapter ({e})");
                None
            }
        })
        .as_ref()
}

/// Drawn at the window's scale, into pixels the plugin's window can show:
/// the header's own colour at its edge, and the preset's name somewhere in
/// the middle of it.
#[test]
fn the_strip_is_drawn_into_pixels_at_the_windows_scale() {
    let Some(headless) = headless() else {
        return;
    };
    let theme = Theme::dark_default();
    let mut labels = fontelle_ui::text::Labels::new();
    let mut text = fontelle_ui::TextContext::new();
    let scale = 2.0;
    let width = (640.0 * scale) as u32;
    let height = (PLUGIN_HEADER_HEIGHT * scale) as u32;
    let pixels = fontelle_ui::render::plugin_header_pixels(
        &mut headless.lock().unwrap(),
        &theme,
        &mut labels,
        &mut text,
        &a_view(),
        None,
        width,
        height,
        scale,
    )
    .expect("the strip renders");
    assert_eq!(pixels.len(), (width * height * 4) as usize);
    let at = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    let edge = at(2, height / 2);
    let header = theme.palette.panel_header.0;
    assert!(
        edge.iter()
            .zip(header.iter())
            .all(|(a, b)| a.abs_diff(*b) <= 1),
        "the strip's own colour at its edge: {edge:?} against {header:?}"
    );
    let row = height / 2;
    let distinct = (0..width)
        .map(|x| at(x, row))
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert!(
        distinct > 8,
        "the bar is drawn on it: {distinct} colours across"
    );
    if let Some(dir) = std::env::var_os("FONTELLE_UI_DUMP") {
        let path = std::path::Path::new(&dir).join("plugin_header.png");
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
}
