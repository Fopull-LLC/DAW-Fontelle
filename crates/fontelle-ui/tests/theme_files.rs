//! Themes as things people share: the `.fontelletheme` file, the built-in
//! looks, and the backdrop images a theme carries inside itself.
//!
//! > *"the way themes should work is they all boil down to a single
//! > .fontelletheme file that can be easily shared and entered and saved into
//! > the app and added to the users theme library to be swapped between so it
//! > just defines what color pallette the app uses as well as maybe some other
//! > easy visual styling things we could do like saying how rounded corners
//! > are ... maybe allowing users to even put background images behind their
//! > arrangement or different panels"* — Ty, 2026-10-01.
//!
//! The library (where the files live, importing one) is `fontelle-app`'s;
//! this is the format.

use fontelle_ui::theme::{
    Backdrop, BackdropPanel, Color, THEME_EXTENSION, THEME_FORMAT_VERSION, Theme, ThemeError,
};

/// A 2×2 PNG, written by the same crate the window decodes with.
fn tiny_png() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ])
            .unwrap();
    }
    out
}

#[test]
fn the_file_is_called_a_fontelletheme() {
    assert_eq!(THEME_EXTENSION, "fontelletheme");
}

#[test]
fn there_are_several_built_in_looks_each_named_and_legible() {
    let builtins = Theme::builtins();
    assert!(builtins.len() >= 4, "more than the two defaults");
    assert_eq!(
        builtins[0],
        Theme::dark_default(),
        "dark first: the default"
    );
    let mut names: Vec<&str> = builtins.iter().map(|t| t.name.as_str()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), builtins.len(), "every name is its own");
    for theme in &builtins {
        let ratio = contrast(theme.palette.text, theme.palette.panel);
        assert!(ratio >= 4.5, "{}: text on panel {ratio:.2}:1", theme.name);
        let muted = contrast(theme.palette.text_muted, theme.palette.panel);
        assert!(muted >= 3.0, "{}: muted text {muted:.2}:1", theme.name);
        let back = Theme::from_json(&theme.to_json()).expect("reads back");
        assert_eq!(&back, theme);
    }
}

#[test]
fn a_built_in_look_is_rigid_with_square_corners() {
    // *"saying how rounded corners are or something so maybe a theme looks
    // more rigid"* — corners are a token, and one of the built-ins uses it.
    let builtins = Theme::builtins();
    assert!(
        builtins.iter().any(|t| t.metrics.corner_radius == 0.0),
        "a square-cornered look"
    );
    assert!(
        builtins
            .iter()
            .any(|t| t.metrics.corner_radius > Theme::dark_default().metrics.corner_radius),
        "and a softer one"
    );
}

#[test]
fn a_theme_carries_its_backdrop_images_inside_it() {
    let mut theme = Theme::dark_default();
    let backdrop = Backdrop::from_image_bytes(&tiny_png(), 0.3).expect("a PNG is an image");
    theme
        .backdrops
        .set(BackdropPanel::Arrangement, Some(backdrop.clone()));
    let text = theme.to_json();
    assert!(
        !text.contains('/') || !text.contains(".png"),
        "the image is in the file, not a path to one"
    );
    let back = Theme::from_json(&text).expect("reads back");
    assert_eq!(back, theme);
    let got = back
        .backdrops
        .get(BackdropPanel::Arrangement)
        .expect("kept");
    assert_eq!(got.image_bytes().as_deref(), Some(tiny_png().as_slice()));
    assert!((got.opacity - 0.3).abs() < 1e-6);
    let decoded = got.decode().expect("decodes");
    assert_eq!((decoded.width, decoded.height), (2, 2));
    assert_eq!(back.backdrops.get(BackdropPanel::Mixer), None);
}

#[test]
fn a_backdrop_that_is_not_an_image_is_refused_when_it_is_chosen() {
    assert!(matches!(
        Backdrop::from_image_bytes(b"not an image at all", 0.5),
        Err(ThemeError::Format(_))
    ));
}

#[test]
fn a_backdrops_opacity_stays_between_nothing_and_whole() {
    let b = Backdrop::from_image_bytes(&tiny_png(), 7.0).unwrap();
    assert_eq!(b.opacity, 1.0);
    let b = Backdrop::from_image_bytes(&tiny_png(), -1.0).unwrap();
    assert_eq!(b.opacity, 0.0);
}

#[test]
fn a_theme_with_no_backdrops_writes_no_backdrop_section() {
    // A plain theme stays the short, hand-editable file it was.
    let text = Theme::dark_default().to_json();
    assert!(!text.contains("backdrops"), "{text}");
}

#[test]
fn a_theme_from_before_backdrops_still_loads_with_none() {
    let mut json: serde_json::Value =
        serde_json::from_str(&Theme::light_default().to_json()).unwrap();
    json["format_version"] = serde_json::json!(9);
    let read = Theme::from_json(&json.to_string()).expect("a v9 theme loads");
    assert!(read.backdrops.is_empty());
    assert_eq!(read.palette, Theme::light_default().palette);
    const { assert!(THEME_FORMAT_VERSION >= 10) };
}

fn luminance(c: Color) -> f64 {
    let channel = |v: u8| {
        let v = v as f64 / 255.0;
        if v <= 0.039_28 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b, _] = c.0;
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

fn contrast(a: Color, b: Color) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[test]
fn a_jpeg_backdrop_decodes_too() {
    // A wallpaper is usually a JPEG.
    let jpeg = include_bytes!("fixtures/backdrop.jpg");
    let b = Backdrop::from_image_bytes(jpeg, 0.5).expect("a JPEG is an image");
    let image = b.decode().expect("decodes");
    assert_eq!((image.width, image.height), (4, 3));
}

// ---------------------------------------- every section, and how it sits ---
//
// Ty, 2026-10-01: *"pictures seem quite limiting right now they only go
// behind the piano roll and arrangement ... i want themes to be able to get
// detailed in the backgrounds they want to make for each section and even
// have transparency for things that can overlap."*

use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{BackdropFit, backdrop_tiles};

#[test]
fn every_section_of_the_window_can_have_a_picture() {
    let names: Vec<&str> = BackdropPanel::ALL.iter().map(|p| p.label()).collect();
    for wanted in [
        "Window",
        "Transport",
        "Channels",
        "Browser",
        "Arrangement",
        "Piano roll",
        "Mixer",
    ] {
        assert!(names.contains(&wanted), "{wanted} missing from {names:?}");
    }
    let mut theme = Theme::dark_default();
    for panel in BackdropPanel::ALL {
        theme.backdrops.set(
            panel,
            Some(Backdrop::from_image_bytes(&tiny_png(), 0.5).unwrap()),
        );
    }
    let back = Theme::from_json(&theme.to_json()).unwrap();
    for panel in BackdropPanel::ALL {
        assert!(back.backdrops.get(panel).is_some(), "{panel:?} lost");
    }
}

#[test]
fn a_picture_written_without_a_fit_covers_from_the_centre() {
    let mut json: serde_json::Value = serde_json::from_str(
        &{
            let mut t = Theme::dark_default();
            t.backdrops.set(
                BackdropPanel::Mixer,
                Some(Backdrop::from_image_bytes(&tiny_png(), 0.5).unwrap()),
            );
            t
        }
        .to_json(),
    )
    .unwrap();
    // Since v11 a section is a stack; the picture is its one layer.
    let mixer = json["backdrops"]["mixer"][0].as_object_mut().unwrap();
    mixer.remove("fit");
    mixer.remove("anchor");
    let read = Theme::from_json(&json.to_string()).unwrap();
    let b = read.backdrops.get(BackdropPanel::Mixer).unwrap();
    assert_eq!(b.fit, BackdropFit::Cover);
    assert_eq!(b.anchor, [0.5, 0.5]);
}

fn area() -> Rect {
    Rect::new(100.0, 50.0, 400.0, 100.0)
}

#[test]
fn cover_fills_the_area_and_the_anchor_says_which_part_shows() {
    // A square picture over a 4:1 panel: scaled to the width, its middle
    // shows by default, its top with the anchor at the top.
    let middle = backdrop_tiles(area(), 200.0, 200.0, BackdropFit::Cover, [0.5, 0.5]);
    assert_eq!(middle, vec![Rect::new(100.0, -100.0, 400.0, 400.0)]);
    let top = backdrop_tiles(area(), 200.0, 200.0, BackdropFit::Cover, [0.5, 0.0]);
    assert_eq!(top, vec![Rect::new(100.0, 50.0, 400.0, 400.0)]);
}

#[test]
fn contain_shows_all_of_it_where_the_anchor_puts_it() {
    let right = backdrop_tiles(area(), 200.0, 200.0, BackdropFit::Contain, [1.0, 0.5]);
    assert_eq!(right, vec![Rect::new(400.0, 50.0, 100.0, 100.0)]);
}

#[test]
fn stretch_is_the_area_itself() {
    let s = backdrop_tiles(area(), 7.0, 3.0, BackdropFit::Stretch, [0.2, 0.9]);
    assert_eq!(s, vec![area()]);
}

#[test]
fn tile_repeats_the_picture_at_its_own_size_over_the_whole_area() {
    let tiles = backdrop_tiles(area(), 64.0, 64.0, BackdropFit::Tile, [0.0, 0.0]);
    // 400 / 64 → 7 across, 100 / 64 → 2 down, from the area's corner.
    assert_eq!(tiles.len(), 7 * 2);
    assert_eq!(tiles[0], Rect::new(100.0, 50.0, 64.0, 64.0));
    for t in &tiles {
        assert!(t.x < area().right() && t.y < area().bottom());
    }
    assert!(
        backdrop_tiles(area(), 0.0, 10.0, BackdropFit::Tile, [0.0, 0.0]).is_empty(),
        "an empty picture is nothing, not an endless loop"
    );
}

#[test]
fn the_time_bars_groove_is_see_through() {
    // *"the time bar at the top should be semi transparent in general
    // instead of being a solid color."*
    for theme in Theme::builtins() {
        let ink = fontelle_ui::render::ruler_track_ink(&theme.palette);
        assert!(
            ink.0[3] < 0xff && ink.0[3] > 0x30,
            "{}: {ink:?}",
            theme.name
        );
    }
}

/// Ember is the built-in that shows what a theme can do: something behind
/// every section, and panels the window's backdrop shows through. Ty:
/// *"revise the ember ... to be more detailed and flashy to showcase the
/// visual design flexibility."* Since hub card 0366 its fire moves: a
/// shader behind the window, its heat and coals still pictures.
#[test]
fn ember_has_a_layer_in_every_section_and_see_through_panels() {
    let ember = Theme::ember();
    for panel in BackdropPanel::ALL {
        assert!(
            !ember.backdrops.layers(panel).is_empty(),
            "{panel:?} has nothing behind it"
        );
        if let Some(b) = ember.backdrops.get(panel) {
            assert!(b.decode().is_some(), "{panel:?}'s picture decodes");
        }
    }
    assert!(ember.backdrops.is_animated());
    assert!(
        ember.palette.panel.0[3] < 0xff,
        "the panels are see-through"
    );
    assert!(
        ember.palette.panel.0[3] >= 0xa0,
        "and still something to read on"
    );
    assert_eq!(ember.palette.window.0[3], 0xff);
}

#[test]
fn what_is_drawn_over_other_content_gets_solid_grounds() {
    // A see-through settings page over the start menu was two layers of
    // words on top of each other.
    let ember = Theme::ember();
    let solid = ember.palette.solid();
    for c in [solid.window, solid.panel, solid.panel_header] {
        assert_eq!(c.0[3], 0xff, "{c:?}");
    }
    assert_eq!(
        solid.panel.0[..3],
        ember.palette.panel.0[..3],
        "same colour"
    );
    assert_eq!(solid.accent, ember.palette.accent, "inks untouched");
}

/// Ty, 2026-10-02: *"by default can you go ahead and make the arrangement
/// time bar semi transparent. it kind of blends with the rest of the menus
/// right now instead of seeming like part of the arrangement window"*. It
/// was the header's own solid ink.
#[test]
fn the_arrangements_time_bar_is_see_through_and_not_the_headers_ink() {
    for theme in Theme::builtins() {
        let ink = fontelle_ui::render::timeline_ruler_ink(&theme.palette);
        assert!(
            ink.0[3] >= 0x30 && ink.0[3] <= 0xa0,
            "{}: {ink:?}",
            theme.name
        );
        assert_ne!(ink, theme.palette.panel_header, "{}", theme.name);
    }
}

// ------------------------------------------- how a picture behaves (v11) ---
//
// Ty, 2026-10-02: *"im noticing that the piano roll images stretch out. this
// should be configurable for people making their themes how their images
// behaive ... we want to offer maximum customization."* Floptle's image
// layer has a natural size, a scale and an offset; so does this one now.

use fontelle_ui::theme::{Layer, backdrop_tiles_scaled};

#[test]
fn natural_size_draws_a_picture_at_its_own_size_where_the_anchor_puts_it() {
    let r = backdrop_tiles_scaled(
        area(),
        50.0,
        20.0,
        BackdropFit::Natural,
        [1.0, 1.0],
        1.0,
        [0.0, 0.0],
    );
    assert_eq!(r, vec![Rect::new(450.0, 130.0, 50.0, 20.0)]);
}

#[test]
fn scale_sizes_a_fitted_picture_and_offset_moves_it() {
    // Contained (100x100 in a 400x100 area), at half size, bottom right,
    // nudged 10 points left and 4 up.
    let r = backdrop_tiles_scaled(
        area(),
        200.0,
        200.0,
        BackdropFit::Contain,
        [1.0, 1.0],
        0.5,
        [-10.0, -4.0],
    );
    assert_eq!(r, vec![Rect::new(440.0, 96.0, 50.0, 50.0)]);
    // A tiled pattern's tiles are scaled too.
    let tiles = backdrop_tiles_scaled(
        area(),
        64.0,
        64.0,
        BackdropFit::Tile,
        [0.0, 0.0],
        0.5,
        [0.0, 0.0],
    );
    assert_eq!(tiles[0], Rect::new(100.0, 50.0, 32.0, 32.0));
    // Stretch ignores the scale: it is the area, as the word says.
    let s = backdrop_tiles_scaled(
        area(),
        7.0,
        3.0,
        BackdropFit::Stretch,
        [0.5, 0.5],
        2.0,
        [0.0, 0.0],
    );
    assert_eq!(s, vec![area()]);
}

#[test]
fn a_picture_keeps_its_size_offset_and_pixel_look_in_the_file() {
    let mut b = Backdrop::from_image_bytes(&tiny_png(), 0.5).unwrap();
    assert_eq!((b.scale, b.offset, b.pixelated), (1.0, [0.0, 0.0], false));
    b.fit = BackdropFit::Natural;
    b.scale = 0.4;
    b.offset = [-20.0, -16.0];
    b.pixelated = true;
    let mut theme = Theme::dark_default();
    theme.backdrops.set(BackdropPanel::Roll, Some(b.clone()));
    let text = theme.to_json();
    assert!(text.contains("\"natural\""), "{text}");
    let back = Theme::from_json(&text).unwrap();
    assert_eq!(
        back.backdrops.layers(BackdropPanel::Roll),
        &[Layer::Image(b)][..]
    );
}
