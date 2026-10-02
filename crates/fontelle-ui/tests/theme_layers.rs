//! Theme format v11: a stack of layers behind each section, moving pictures
//! drawn by a WGSL shader among them, and a font carried inside the file.
//!
//! Hub card 0366: *"Fontelle themes can carry animated WGSL shader backdrops,
//! like Floptle's"* — and the theme is still **one shareable file**, so the
//! shader's text and the font's bytes ride inside it, as v10's pictures do.
//!
//! The pictures and how they sit are `theme_files.rs`; the shaders' contract
//! and the renderer are `backdrop_shaders.rs`.

use fontelle_ui::theme::{
    Backdrop, BackdropPanel, Blend, GradientLayer, Layer, ShaderLayer, THEME_FORMAT_VERSION, Theme,
    ThemeError, ThemeFont,
};

fn tiny_png() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255; 16]).unwrap();
    }
    out
}

fn a_shader(source: &str) -> Layer {
    Layer::Shader(ShaderLayer::new(source))
}

#[test]
fn the_format_moved_on_for_layers() {
    const { assert!(THEME_FORMAT_VERSION >= 11) };
}

#[test]
fn a_section_holds_a_stack_of_layers_bottom_to_top() {
    let mut theme = Theme::dark_default();
    let picture = Backdrop::from_image_bytes(&tiny_png(), 0.4).unwrap();
    let stack = vec![
        a_shader("builtin:aurora"),
        Layer::Image(picture.clone()),
        Layer::Gradient(GradientLayer {
            stops: vec![
                (0.0, theme.palette.accent),
                (1.0, theme.palette.playhead.with_alpha(0)),
            ],
            angle: 90.0,
            radial: false,
            opacity: 0.5,
            blend: Blend::Add,
        }),
        Layer::Solid {
            color: theme.palette.window.with_alpha(0x80),
        },
    ];
    *theme.backdrops.layers_mut(BackdropPanel::Arrangement) = stack.clone();
    let back = Theme::from_json(&theme.to_json()).expect("reads back");
    assert_eq!(back, theme);
    assert_eq!(
        back.backdrops.layers(BackdropPanel::Arrangement),
        &stack[..]
    );
    assert!(back.backdrops.layers(BackdropPanel::Mixer).is_empty());
    // The picture accessor still finds the picture among the layers.
    assert_eq!(
        back.backdrops.get(BackdropPanel::Arrangement),
        Some(&picture)
    );
}

#[test]
fn a_shader_written_inline_travels_inside_the_file() {
    let wgsl = "fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {\n    \
                return vec4<f32>(uv, 0.5 + 0.5 * sin(bd.time), 1.0);\n}\n";
    let mut theme = Theme::dark_default();
    theme
        .backdrops
        .layers_mut(BackdropPanel::Window)
        .push(a_shader(wgsl));
    let text = theme.to_json();
    assert!(text.contains("sin(bd.time)"), "the WGSL is in the file");
    let back = Theme::from_json(&text).unwrap();
    let Layer::Shader(s) = &back.backdrops.layers(BackdropPanel::Window)[0] else {
        panic!("not a shader layer");
    };
    assert_eq!(s.shader, wgsl);
    assert_eq!((s.opacity, s.speed, s.scale), (1.0, 1.0, 1.0));
    assert_eq!(s.blend, Blend::Normal);
}

#[test]
fn a_shader_layer_written_with_only_its_shader_takes_the_defaults() {
    let mut json: serde_json::Value =
        serde_json::from_str(&Theme::dark_default().to_json()).unwrap();
    json["backdrops"] = serde_json::json!({
        "window": [ { "kind": "shader", "shader": "builtin:drift" } ]
    });
    let theme = Theme::from_json(&json.to_string()).expect("a short shader layer reads");
    let Layer::Shader(s) = &theme.backdrops.layers(BackdropPanel::Window)[0] else {
        panic!()
    };
    assert_eq!(s.shader, "builtin:drift");
    assert!(s.colors.is_empty() && s.params.is_empty() && s.image.is_none());
    // Unset colours are the theme's own: accent, playhead, window, text.
    let p = &theme.palette;
    assert_eq!(s.colors_for(p), [p.accent, p.playhead, p.window, p.text]);
}

#[test]
fn a_shader_with_too_many_colours_or_numbers_is_refused_by_name() {
    let mut json: serde_json::Value =
        serde_json::from_str(&Theme::dark_default().to_json()).unwrap();
    json["backdrops"] = serde_json::json!({
        "roll": [ { "kind": "shader", "shader": "builtin:drift",
                    "colors": ["#000000", "#111111", "#222222", "#333333", "#444444"] } ]
    });
    let Err(ThemeError::Format(why)) = Theme::from_json(&json.to_string()) else {
        panic!("five colours read");
    };
    assert!(why.contains("colours"), "{why}");
    json["backdrops"] = serde_json::json!({
        "roll": [ { "kind": "shader", "shader": "builtin:drift", "params": [0,1,2,3,4,5,6,7,8] } ]
    });
    let Err(ThemeError::Format(why)) = Theme::from_json(&json.to_string()) else {
        panic!("nine numbers read");
    };
    assert!(why.contains("numbers"), "{why}");
}

#[test]
fn an_unknown_layer_kind_or_key_is_refused_by_name() {
    let mut json: serde_json::Value =
        serde_json::from_str(&Theme::dark_default().to_json()).unwrap();
    json["backdrops"] = serde_json::json!({ "window": [ { "kind": "video", "path": "x.mp4" } ] });
    assert!(Theme::from_json(&json.to_string()).is_err());
    json["backdrops"] =
        serde_json::json!({ "window": [ { "kind": "shader", "shader": "x", "sped": 2.0 } ] });
    let Err(ThemeError::Format(why)) = Theme::from_json(&json.to_string()) else {
        panic!("a typo read");
    };
    assert!(why.contains("sped"), "{why}");
}

/// A v10 file — Ember's shape, and Ty's own file — still loads, each section's
/// one picture becoming one picture layer, and looks the same.
#[test]
fn a_version_10_file_still_loads_its_pictures_as_picture_layers() {
    let mut theme = Theme::dark_default();
    let picture = Backdrop::from_image_bytes(&tiny_png(), 0.6).unwrap();
    *theme.backdrops.layers_mut(BackdropPanel::Mixer) = vec![Layer::Image(picture.clone())];
    // Written as v10 wrote it: the section is the picture itself.
    let mut json: serde_json::Value = serde_json::from_str(&theme.to_json()).unwrap();
    json["format_version"] = serde_json::json!(10);
    json["backdrops"] = serde_json::json!({
        "mixer": {
            "image": picture.image,
            "opacity": 0.6,
            "fit": "contain",
            "anchor": [1.0, 0.0]
        }
    });
    let read = Theme::from_json(&json.to_string()).expect("a v10 theme loads");
    assert_eq!(read.format_version, THEME_FORMAT_VERSION);
    let layers = read.backdrops.layers(BackdropPanel::Mixer);
    assert_eq!(layers.len(), 1);
    let Layer::Image(b) = &layers[0] else {
        panic!("not a picture")
    };
    assert_eq!(b.image, picture.image);
    assert_eq!(b.opacity, 0.6);
    assert_eq!(b.fit, fontelle_ui::theme::BackdropFit::Contain);
    assert_eq!(b.anchor, [1.0, 0.0]);
    assert!(!read.backdrops.is_animated());
}

#[test]
fn a_theme_from_a_newer_build_is_still_refused_by_version() {
    let mut json: serde_json::Value =
        serde_json::from_str(&Theme::dark_default().to_json()).unwrap();
    json["format_version"] = serde_json::json!(THEME_FORMAT_VERSION + 1);
    assert!(matches!(
        Theme::from_json(&json.to_string()),
        Err(ThemeError::FromTheFuture { .. })
    ));
}

#[test]
fn setting_a_picture_keeps_the_sections_moving_layers() {
    let mut theme = Theme::dark_default();
    theme
        .backdrops
        .layers_mut(BackdropPanel::Roll)
        .push(a_shader("builtin:waves"));
    let picture = Backdrop::from_image_bytes(&tiny_png(), 0.5).unwrap();
    theme
        .backdrops
        .set(BackdropPanel::Roll, Some(picture.clone()));
    let layers = theme.backdrops.layers(BackdropPanel::Roll);
    assert_eq!(layers.len(), 2, "{layers:?}");
    assert!(
        matches!(layers[0], Layer::Shader(_)),
        "the shader stays under"
    );
    assert_eq!(layers[1], Layer::Image(picture));
    theme.backdrops.set(BackdropPanel::Roll, None);
    assert_eq!(theme.backdrops.layers(BackdropPanel::Roll).len(), 1);
    assert!(theme.backdrops.get(BackdropPanel::Roll).is_none());
}

#[test]
fn a_theme_moves_only_when_one_of_its_shaders_has_a_speed() {
    let mut theme = Theme::dark_default();
    assert!(!theme.backdrops.is_animated());
    let mut still = ShaderLayer::new("builtin:drift");
    still.speed = 0.0;
    theme
        .backdrops
        .layers_mut(BackdropPanel::Window)
        .push(Layer::Shader(still));
    assert!(!theme.backdrops.is_animated(), "speed 0 holds one frame");
    assert!(theme.backdrops.has_shaders());
    theme
        .backdrops
        .layers_mut(BackdropPanel::Transport)
        .push(a_shader("builtin:drift"));
    assert!(theme.backdrops.is_animated());
}

// ------------------------------------------------------------- fonts ---

/// The bundled Open Sans: a real TrueType file to carry.
fn a_font() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fonts/OpenSans-Regular.ttf"
    ))
    .unwrap()
}

#[test]
fn a_theme_can_carry_its_own_font_inside_the_file() {
    let mut theme = Theme::dark_default();
    let font = ThemeFont::from_bytes(&a_font()).expect("a TrueType file is a font");
    theme.fonts.push(font.clone());
    let back = Theme::from_json(&theme.to_json()).unwrap();
    assert_eq!(back.fonts, vec![font]);
    assert_eq!(back.fonts[0].bytes().as_deref(), Some(a_font().as_slice()));
}

#[test]
fn a_font_that_is_not_one_is_refused_when_it_is_chosen() {
    assert!(ThemeFont::from_bytes(b"definitely not a font").is_err());
    assert!(ThemeFont::from_bytes(&tiny_png()).is_err());
}

#[test]
fn a_plain_theme_still_writes_no_fonts_layers_or_description() {
    let text = Theme::dark_default().to_json();
    for absent in ["\"fonts\"", "\"backdrops\"", "\"display\""] {
        assert!(!text.contains(absent), "{absent} in {text}");
    }
}

#[test]
fn the_window_loads_a_themes_font_and_draws_with_it() {
    let mut text = fontelle_ui::text::TextContext::new();
    let mut theme = Theme::dark_default();
    theme.fonts.push(ThemeFont::from_bytes(&a_font()).unwrap());
    let families = text.load_theme_fonts(&theme);
    assert!(
        families.iter().any(|f| f == "Open Sans"),
        "the font's own family name is what a theme names: {families:?}"
    );
    // Loading the same theme twice adds nothing the second time.
    assert!(text.load_theme_fonts(&theme).is_empty());
    theme.font.family = "Open Sans".to_string();
    let shaped = text.layout("Fontelle", &theme.font, None);
    assert!(shaped.glyph_count() > 0);
}
