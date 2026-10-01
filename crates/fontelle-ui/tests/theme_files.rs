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
