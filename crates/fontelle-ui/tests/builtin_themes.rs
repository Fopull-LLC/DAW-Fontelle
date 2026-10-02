//! The looks Fontelle ships (hub card 0366): the five it had, each improved,
//! and the animated ones added for different tastes — calm, light,
//! dark-studio, playful, retro — plus a high-contrast pair.

use fontelle_ui::theme::{BackdropPanel, Color, Layer, Theme};

fn named(name: &str) -> Theme {
    Theme::builtins()
        .into_iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no built-in called {name:?}"))
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

/// What a see-through panel actually is on screen: its colour over the
/// window's, at its alpha. The words sit on this, not on the token.
fn over_window(theme: &Theme, c: Color) -> Color {
    let a = c.0[3] as f64 / 255.0;
    let w = theme.palette.window.0;
    let mix = |i: usize| (c.0[i] as f64 * a + w[i] as f64 * (1.0 - a)).round() as u8;
    Color::rgb(mix(0), mix(1), mix(2))
}

#[test]
fn every_built_in_has_a_line_saying_what_it_is() {
    for theme in Theme::builtins() {
        let line = theme.description.trim();
        assert!(!line.is_empty(), "{} has no description", theme.name);
        assert!(
            line.len() <= 90,
            "{}: one line, not {}",
            theme.name,
            line.len()
        );
        assert!(!line.contains('\n'), "{}", theme.name);
    }
}

#[test]
fn the_names_and_order_are_stable_with_the_default_first() {
    let names: Vec<String> = Theme::builtins().into_iter().map(|t| t.name).collect();
    assert_eq!(names[0], "Fontelle Dark");
    for kept in ["Fontelle Light", "Midnight", "Ember", "Paper"] {
        assert!(names.iter().any(|n| n == kept), "{kept} kept");
    }
}

#[test]
fn at_least_eight_new_built_ins_move() {
    let old = [
        "Fontelle Dark",
        "Fontelle Light",
        "Midnight",
        "Ember",
        "Paper",
    ];
    let moving: Vec<String> = Theme::builtins()
        .into_iter()
        .filter(|t| !old.contains(&t.name.as_str()) && t.backdrops.is_animated())
        .map(|t| t.name)
        .collect();
    assert!(moving.len() >= 8, "{moving:?}");
}

#[test]
fn the_new_ones_cover_light_and_dark_tastes() {
    let moving: Vec<Theme> = Theme::builtins()
        .into_iter()
        .filter(|t| t.backdrops.is_animated())
        .collect();
    let light = moving
        .iter()
        .filter(|t| luminance(t.palette.window) > 0.4)
        .count();
    assert!(light >= 1, "people in bright rooms get motion too");
    assert!(moving.len() - light >= 6, "and the dark studios");
}

#[test]
fn the_default_stays_still_and_sober() {
    let dark = Theme::dark_default();
    assert!(!dark.backdrops.is_animated());
    assert!(!dark.backdrops.has_shaders());
    let p = &dark.palette;
    assert!(contrast(p.text, p.panel) >= 7.0, "AAA for body text");
    assert!(
        contrast(p.text_muted, p.panel) >= 4.5,
        "AA for the quiet text"
    );
}

#[test]
fn fontelle_light_reads_at_aa_for_every_ink_of_text() {
    let light = named("Fontelle Light");
    let p = &light.palette;
    assert!(contrast(p.text, p.panel) >= 7.0);
    assert!(contrast(p.text_muted, p.panel) >= 4.5);
    assert!(contrast(p.text_muted, p.panel_header) >= 4.5);
}

#[test]
fn midnight_and_ember_move_now() {
    for name in ["Midnight", "Ember"] {
        assert!(named(name).backdrops.is_animated(), "{name}");
    }
    // Ember keeps a picture or a layer in every section.
    let ember = named("Ember");
    for panel in BackdropPanel::ALL {
        assert!(
            !ember.backdrops.layers(panel).is_empty(),
            "Ember's {panel:?} is bare"
        );
    }
}

#[test]
fn the_high_contrast_pair_is_still_opaque_and_very_legible() {
    for name in ["High Contrast", "High Contrast Light"] {
        let t = named(name);
        assert!(
            !t.backdrops.has_shaders(),
            "{name}: nothing moves behind text"
        );
        let p = &t.palette;
        for c in [p.window, p.panel, p.panel_header] {
            assert_eq!(c.0[3], 0xff, "{name}: opaque grounds");
        }
        assert!(contrast(p.text, p.panel) >= 15.0, "{name}");
        assert!(contrast(p.text_muted, p.panel) >= 7.0, "{name}");
        assert!(
            contrast(p.accent, p.panel) >= 4.5,
            "{name}: the accent reads"
        );
        assert!(t.metrics.border_width >= 2.0, "{name}: thicker borders");
        assert!(
            t.font.size > Theme::dark_default().font.size,
            "{name}: larger text"
        );
        assert!(!t.fonts.is_empty(), "{name}: carries its legible face");
    }
}

#[test]
fn every_built_in_s_text_reads_on_what_it_actually_sits_on() {
    for theme in Theme::builtins() {
        let p = &theme.palette;
        let panel = over_window(&theme, p.panel);
        let header = over_window(&theme, p.panel_header);
        let body = contrast(p.text, panel);
        assert!(body >= 4.5, "{}: text on panel {body:.2}", theme.name);
        let quiet = contrast(p.text_muted, panel);
        assert!(quiet >= 3.0, "{}: muted on panel {quiet:.2}", theme.name);
        let head = contrast(p.text, header);
        assert!(head >= 4.5, "{}: text on header {head:.2}", theme.name);
    }
}

#[test]
fn the_text_panels_veils_keep_the_grid_countable() {
    // Card 0366's craft note: text panels' veils at 0.75–0.95 alpha.
    for theme in Theme::builtins() {
        let a = theme.palette.panel.0[3];
        assert!(a >= 0xbf, "{}: panel alpha {a:#x}", theme.name);
    }
}

#[test]
fn stage_is_the_night_stage_with_its_own_rounded_faces() {
    let stage = named("Stage");
    let p = &stage.palette;
    assert_eq!(p.accent, Color::rgb(0x39, 0xc5, 0xbb), "teal");
    assert_eq!(p.playhead, Color::rgb(0xff, 0x4f, 0xa3), "pink");
    assert_eq!(&p.selection.0[..3], &[0xff, 0x4f, 0xa3], "pink selection");
    assert_eq!(p.note, Color::rgb(0x39, 0xc5, 0xbb), "teal notes");
    assert_eq!(stage.metrics.corner_radius, 12.0);
    assert_eq!(stage.font.family, "Rounded Mplus 1c");
    assert_eq!(stage.font.display.as_deref(), Some("Mochiy Pop One"));
    assert_eq!(stage.fonts.len(), 2);
    // One stage shader shared by every section that shows one: one texture.
    let shaders: Vec<&str> = BackdropPanel::ALL
        .iter()
        .flat_map(|panel| stage.backdrops.layers(*panel))
        .filter_map(|l| match l {
            Layer::Shader(s) => Some(s.shader.as_str()),
            _ => None,
        })
        .collect();
    assert!(!shaders.is_empty());
    assert!(shaders.iter().all(|s| *s == "builtin:stage"), "{shaders:?}");
    // And no picture: the character art is the user's to add.
    for panel in BackdropPanel::ALL {
        assert!(stage.backdrops.get(panel).is_none(), "{panel:?}");
    }
}

#[test]
fn every_built_in_reads_back_from_its_own_file() {
    for theme in Theme::builtins() {
        let back = Theme::from_json(&theme.to_json()).expect("reads back");
        assert_eq!(back, theme, "{}", theme.name);
    }
}
