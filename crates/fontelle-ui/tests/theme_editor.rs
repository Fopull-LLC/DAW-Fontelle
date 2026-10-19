//! Making a theme in the studio (the next update, phase 5): every colour of
//! the palette by name, a colour picker that reads and writes them, and a
//! card beside the studio laid out from rectangles.

use fontelle_ui::backdrop::shader_problem;
use fontelle_ui::canvas::{
    ShaderEditorHit, ThemeEditorHit, ThemeEditorTab, hue_at, palette_group, picker_colour,
    picker_point, shader_editor_hit, shader_editor_layout, theme_editor_hit, theme_editor_layout,
};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Color, Theme};

#[test]
fn every_palette_colour_is_reachable_by_name() {
    let mut palette = Theme::dark_default().palette;
    let names = palette.names();
    assert!(names.len() >= 35, "{}", names.len());
    assert!(names.iter().any(|n| n == "accent"));
    for name in &names {
        let c = palette.get(name).expect(name);
        palette.set(name, Color::rgb(1, 2, 3)).unwrap();
        assert_eq!(palette.get(name), Some(Color::rgb(1, 2, 3)));
        palette.set(name, c).unwrap();
    }
    assert!(palette.set("not a colour", Color::rgb(0, 0, 0)).is_err());
    // Each has a group to be listed under.
    assert!(names.iter().all(|n| !palette_group(n).is_empty()));
    assert_eq!(palette_group("accent"), palette_group("selection"));
}

#[test]
fn a_colour_goes_round_hue_saturation_value_and_back() {
    for c in [
        Color::rgb(255, 0, 0),
        Color::rgb(0, 128, 255),
        Color::rgba(12, 200, 90, 128),
        Color::rgb(30, 30, 30),
        Color::rgb(255, 255, 255),
    ] {
        let [h, s, v, a] = c.to_hsva();
        let back = Color::from_hsva(h, s, v, a);
        for i in 0..4 {
            assert!(
                (i16::from(back.0[i]) - i16::from(c.0[i])).abs() <= 1,
                "{c:?} -> {back:?}"
            );
        }
    }
    assert!((Color::rgb(0, 255, 0).to_hsva()[0] - 120.0).abs() < 0.5);
}

/// The square picks saturation across and value up; the strip, hue down.
#[test]
fn the_picker_reads_a_point_and_shows_where_a_colour_is() {
    let square = Rect::new(10.0, 10.0, 200.0, 100.0);
    let hue = Rect::new(220.0, 10.0, 16.0, 100.0);
    let red = Color::rgb(255, 0, 0);
    // Top right of the square at hue 0 is pure red.
    let c = picker_colour(square, 0.0, 1.0, square.right() - 0.01, square.y + 0.01);
    assert!(c.0[0] > 250 && c.0[1] < 5 && c.0[2] < 5, "{c:?}");
    // Bottom is black whatever the hue.
    let c = picker_colour(square, 200.0, 1.0, square.x + 50.0, square.bottom());
    assert_eq!([c.0[0], c.0[1], c.0[2]], [0, 0, 0]);
    let (x, y) = picker_point(square, red);
    assert!((x - square.right()).abs() < 1.0 && (y - square.y).abs() < 1.0);
    assert!(hue_at(hue, hue.y).abs() < 0.5);
    assert!((hue_at(hue, hue.y + hue.height / 2.0) - 180.0).abs() < 0.5);
    assert!(hue_at(hue, hue.bottom() + 40.0) <= 360.0, "clamped");
}

#[test]
fn the_card_sits_beside_the_studio_with_its_parts_inside() {
    let window = Rect::new(0.0, 0.0, 1400.0, 900.0);
    let metrics = Theme::dark_default().metrics;
    let l = theme_editor_layout(window, &metrics, ThemeEditorTab::Colours, 45, 0);
    assert!(l.frame.x > window.width / 2.0, "it covers the studio");
    assert!(l.frame.intersection(&window) == l.frame);
    for part in [
        l.save, l.close, l.square, l.hue, l.alpha, l.hex, l.tabs[0], l.tabs[1],
    ] {
        assert!(!part.is_empty());
        assert!(part.intersection(&l.frame) == part, "{part:?} outside");
    }
    assert!(!l.rows.is_empty());
    let (x, y) = (l.rows[0].x + 5.0, l.rows[0].y + 5.0);
    assert_eq!(theme_editor_hit(&l, x, y), Some(ThemeEditorHit::Row(0)));
    let (x, y) = (l.save.x + 2.0, l.save.y + 2.0);
    assert_eq!(theme_editor_hit(&l, x, y), Some(ThemeEditorHit::Save));
    assert_eq!(
        theme_editor_hit(&l, l.square.x + 5.0, l.square.y + 5.0),
        Some(ThemeEditorHit::Square)
    );
    assert_eq!(theme_editor_hit(&l, 5.0, 5.0), None, "off the card");
}

/// The backdrops tab: the seven sections, a section's layers, the ways to
/// add one, and the selected layer's own controls.
#[test]
fn the_backdrops_tab_lists_a_sections_layers() {
    let window = Rect::new(0.0, 0.0, 1400.0, 900.0);
    let metrics = Theme::dark_default().metrics;
    let l = theme_editor_layout(window, &metrics, ThemeEditorTab::Backdrops, 0, 3);
    assert_eq!(l.sections.len(), 7);
    assert_eq!(l.layers.len(), 3);
    assert_eq!(l.add.len(), 3, "a colour, a gradient, a shader");
    for (i, layer) in l.layers.iter().enumerate() {
        let (x, y) = (layer.row.x + 5.0, layer.row.y + layer.row.height / 2.0);
        assert_eq!(theme_editor_hit(&l, x, y), Some(ThemeEditorHit::Layer(i)));
        let (x, y) = (layer.remove.x + 2.0, layer.remove.y + 2.0);
        assert_eq!(
            theme_editor_hit(&l, x, y),
            Some(ThemeEditorHit::RemoveLayer(i))
        );
    }
}

/// A compile error is said against the line the author wrote, not the
/// line of the whole program Fontelle wraps it in.
#[test]
fn a_shader_error_points_at_the_authors_line() {
    let good = "fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {\n    return vec4<f32>(uv, 0.0, 1.0);\n}\n";
    assert_eq!(shader_problem(good), None);
    let bad = "fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {\n    let a = 1.0;\n    return vec4<f32>(uv, nope, 1.0);\n}\n";
    let p = shader_problem(bad).expect("it does not compile");
    assert_eq!(p.line, Some(3), "{p:?}");
    assert!(!p.message.contains('\n') && !p.message.is_empty(), "{p:?}");
    let none = shader_problem("fn other() {}").expect("no backdrop");
    assert_eq!(none.line, None);
}

/// The code card takes the studio's side of the window, leaving the theme
/// card beside it, with the text, the error line and Done.
#[test]
fn the_code_card_leaves_the_theme_card_alone() {
    let window = Rect::new(0.0, 0.0, 1400.0, 900.0);
    let metrics = Theme::dark_default().metrics;
    let card = theme_editor_layout(window, &metrics, ThemeEditorTab::Backdrops, 0, 1);
    let l = shader_editor_layout(window, card.frame, &metrics);
    assert!(!l.frame.intersects(&card.frame));
    for part in [l.text, l.problem, l.done] {
        assert!(!part.is_empty());
        assert!(part.intersection(&l.frame) == part);
    }
    assert!(!l.text.intersects(&l.problem));
    assert_eq!(
        shader_editor_hit(&l, l.done.x + 2.0, l.done.y + 2.0),
        Some(ShaderEditorHit::Done)
    );
    assert_eq!(
        shader_editor_hit(&l, l.text.x + 5.0, l.text.y + 5.0),
        Some(ShaderEditorHit::Text)
    );
}
