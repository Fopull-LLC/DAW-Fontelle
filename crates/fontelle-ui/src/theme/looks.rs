//! The built-in looks added with moving backdrops (hub card 0366), each
//! chosen for a different taste: calm, light, dark-studio, playful, retro,
//! and a high-contrast pair.
//!
//! Every one is a whole [`Palette`] — the closed list the format insists on
//! — written from a handful of inks by [`inks`], so that a look is the six
//! or seven colours that make it and not thirty lines of arithmetic. Each
//! one's text is held to WCAG against what its panels actually composite to
//! by `tests/builtin_themes.rs`.

use super::{
    Backdrops, Blend, Color, FontTokens, GradientLayer, Layer, METRICS, Palette, ShaderLayer,
    Theme, ThemeFont,
};

/// `a` toward `b` by `t`, opaque.
pub(super) fn mix(a: Color, b: Color, t: f32) -> Color {
    let m = |i: usize| (a.0[i] as f32 + (b.0[i] as f32 - a.0[i] as f32) * t).round() as u8;
    Color::rgb(m(0), m(1), m(2))
}

pub(super) const fn hex(rgb: u32) -> Color {
    Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// The inks a look is made of; everything else is drawn from them.
struct Inks {
    window: Color,
    panel: Color,
    header: Color,
    border: Color,
    text: Color,
    muted: Color,
    accent: Color,
    playhead: Color,
    note: Color,
}

/// A whole palette from `inks`. `light` says which way the rows shade: a
/// dark look's off-rows sink toward its window, a light look's darken.
fn inks(i: &Inks, light: bool) -> Palette {
    let base = if light {
        Theme::light_default().palette
    } else {
        Theme::dark_default().palette
    };
    let white = Color::rgb(0xff, 0xff, 0xff);
    let black = Color::rgb(0, 0, 0);
    Palette {
        window: i.window,
        panel: i.panel,
        panel_header: i.header,
        border: i.border,
        text: i.text,
        text_muted: i.muted,
        accent: i.accent,
        grid_line_sub: mix(i.panel, i.border, 0.3),
        grid_line: mix(i.panel, i.border, 0.65),
        grid_line_strong: i.border,
        playhead: i.playhead,
        selection: i.accent.with_alpha(if light { 0x40 } else { 0x50 }),
        meter: base.meter,
        meter_peak: base.meter_peak,
        note: i.note,
        note_selected: if light {
            mix(i.note, black, 0.35)
        } else {
            mix(i.note, white, 0.55)
        },
        key_white: if light {
            mix(white, i.panel, 0.3)
        } else {
            mix(i.text, i.panel, 0.12)
        },
        key_black: if light {
            mix(i.text, i.panel, 0.2)
        } else {
            i.header
        },
        row_accidental: mix(i.panel, i.window, if light { 0.35 } else { 0.5 }),
        row_dead: if light {
            mix(i.window, black, 0.08)
        } else {
            mix(i.window, black, 0.3)
        },
        key_dead: mix(i.muted, i.panel, 0.45),
        row_out_of_scale: mix(i.panel, i.window, if light { 0.55 } else { 0.7 }),
        row_scale_root: mix(i.panel, i.accent, 0.18),
        note_silent: mix(i.note, i.panel, 0.6),
        param_automated: base.param_automated,
        modulation: base.modulation,
        mod_envelope: base.mod_envelope,
        mod_lfo: base.mod_lfo,
        mod_macro: base.mod_macro,
        mod_note: base.mod_note,
        mod_performance: base.mod_performance,
    }
}

/// See-through panels — `panel` and `header` alphas — over the window's
/// backdrop, with the row inks thinned to match.
fn veil(p: &mut Palette, panel: u8, header: u8) {
    p.panel = p.panel.with_alpha(panel);
    p.panel_header = p.panel_header.with_alpha(header);
    p.row_accidental = p.row_accidental.with_alpha(panel.saturating_sub(0x30));
    p.row_out_of_scale = p.row_out_of_scale.with_alpha(panel.saturating_sub(0x30));
}

fn look(name: &str, description: &str, palette: Palette, radius: f32) -> Theme {
    let mut theme = Theme::dark_default();
    theme.name = name.to_string();
    theme.description = description.to_string();
    theme.palette = palette;
    theme.metrics.corner_radius = radius;
    theme
}

/// One shader layer behind the whole window, in `colors`.
fn ground(theme: &mut Theme, shader: &str, colors: [Color; 4], speed: f32) {
    theme.backdrops.window = vec![Layer::Shader(ShaderLayer {
        colors: colors.to_vec(),
        speed,
        ..ShaderLayer::new(shader)
    })];
}

/// A glow added over a section, from `from` at its top to `to` at its
/// bottom.
pub(super) fn glow(from: Color, to: Color, opacity: f32) -> Layer {
    Layer::Gradient(GradientLayer {
        stops: vec![(0.0, from), (1.0, to)],
        angle: 90.0,
        radial: false,
        opacity,
        blend: Blend::Add,
    })
}

fn font(bytes: &[u8]) -> ThemeFont {
    ThemeFont::from_bytes(bytes).expect("the built-in looks' fonts are compiled in and are fonts")
}

// ------------------------------------------------------------- calm ---

pub(super) fn still_water() -> Theme {
    let mut p = inks(
        &Inks {
            window: hex(0x1a2430),
            panel: hex(0x222d3a),
            header: hex(0x27333f),
            border: hex(0x3a4858),
            text: hex(0xe3e9ef),
            muted: hex(0x98a8b8),
            accent: hex(0x7fb0c8),
            playhead: hex(0xa8d8c8),
            note: hex(0x6d9ab8),
        },
        false,
    );
    veil(&mut p, 0xc8, 0xd8);
    let mut theme = look(
        "Still Water",
        "Slate blue and quiet, with light moving slowly on deep water.",
        p,
        6.0,
    );
    ground(
        &mut theme,
        "builtin:stillwater",
        [hex(0xb8d8e8), hex(0x7f9ec8), hex(0x1c2836), hex(0xe3e9ef)],
        1.0,
    );
    theme
}

pub(super) fn aurora() -> Theme {
    let mut p = inks(
        &Inks {
            window: hex(0x051414),
            panel: hex(0x0a1d1f),
            header: hex(0x0e2528),
            border: hex(0x1d4245),
            text: hex(0xe0f4f1),
            muted: hex(0x8fbab4),
            accent: hex(0x4fd6a8),
            playhead: hex(0x9fb4ff),
            note: hex(0x3fae96),
        },
        false,
    );
    veil(&mut p, 0xcc, 0xdc);
    let mut theme = look(
        "Aurora",
        "Northern lights folding slowly behind deep teal glass.",
        p,
        8.0,
    );
    ground(
        &mut theme,
        "builtin:aurora",
        [hex(0x4fd6a8), hex(0x6a7cff), hex(0x041012), hex(0xe0f4f1)],
        1.0,
    );
    theme
}

// ------------------------------------------------------------ light ---

pub(super) fn sunroom() -> Theme {
    let mut p = inks(
        &Inks {
            window: hex(0xefe6d6),
            panel: hex(0xfaf6ee),
            header: hex(0xf2ebdf),
            border: hex(0xd8ccb6),
            text: hex(0x2b2620),
            muted: hex(0x5e5446),
            accent: hex(0x4f7d3a),
            playhead: hex(0xb4562a),
            note: hex(0x5a7f9c),
        },
        true,
    );
    veil(&mut p, 0xdc, 0xe8);
    let mut theme = look(
        "Sunroom",
        "Daylight on a warm wall, with leaf shadows drifting in a breeze.",
        p,
        8.0,
    );
    ground(
        &mut theme,
        "builtin:sunroom",
        [hex(0x6e7a52), hex(0xfff2c8), hex(0xefe6d6), hex(0x2b2620)],
        1.0,
    );
    theme
}

// ------------------------------------------------------- dark studio ---

pub(super) fn control_room() -> Theme {
    let mut p = inks(
        &Inks {
            window: hex(0x16151a),
            panel: hex(0x1e1d23),
            header: hex(0x24232a),
            border: hex(0x3a3842),
            text: hex(0xece6dc),
            muted: hex(0xa59d90),
            accent: hex(0xf0a63a),
            playhead: hex(0xffd27a),
            note: hex(0xc98a3a),
        },
        false,
    );
    p.meter = hex(0x9cc06a);
    veil(&mut p, 0xc8, 0xd8);
    let mut theme = look(
        "Control Room",
        "A studio after dark: charcoal, foam, lamps, and a VU-amber glow.",
        p,
        4.0,
    );
    ground(
        &mut theme,
        "builtin:controlroom",
        [hex(0xf0a63a), hex(0x8aa6d8), hex(0x1a191f), hex(0xece6dc)],
        1.0,
    );
    theme
}

pub(super) fn nebula() -> Theme {
    let mut p = inks(
        &Inks {
            window: hex(0x070811),
            panel: hex(0x0f1220),
            header: hex(0x141829),
            border: hex(0x2a3050),
            text: hex(0xe6e8f6),
            muted: hex(0x9aa0c4),
            accent: hex(0x8f7ce0),
            playhead: hex(0x5cc8d8),
            note: hex(0x6f7fd0),
        },
        false,
    );
    veil(&mut p, 0xcc, 0xdc);
    let mut theme = look(
        "Nebula",
        "A spiral galaxy turning slowly: the sky Flopsynth's bridge looks out on.",
        p,
        8.0,
    );
    // The bridge's sky inks (`sky::SkyPalette::for_theme` over the bridge's
    // palette): the accent teal and the modulation violet as the two
    // clouds, deep space, and the text's white as starlight.
    let dark = Theme::dark_default().palette;
    ground(
        &mut theme,
        "builtin:galaxy",
        [
            mix(dark.accent, Color::rgb(0xff, 0xff, 0xff), 0.15),
            dark.modulation,
            hex(0x05060c),
            dark.text,
        ],
        1.0,
    );
    theme
}

// ---------------------------------------------------------- playful ---

pub(super) fn lofi_rain() -> Theme {
    let mut p = inks(
        &Inks {
            window: hex(0x0c1018),
            panel: hex(0x141a26),
            header: hex(0x19202d),
            border: hex(0x2c3548),
            text: hex(0xe6e8f0),
            muted: hex(0x9aa3b8),
            accent: hex(0xe8a75c),
            playhead: hex(0x7cc4e8),
            note: hex(0xc08a58),
        },
        false,
    );
    veil(&mut p, 0xc4, 0xd4);
    let mut theme = look(
        "Lo-fi Rain",
        "City lights blurred behind a rainy window at night.",
        p,
        10.0,
    );
    ground(
        &mut theme,
        "builtin:rain",
        [hex(0xffb35c), hex(0xff5c9a), hex(0x0e1320), hex(0x8cc8ff)],
        1.0,
    );
    theme
}

pub(super) fn bubblegum() -> Theme {
    let mut p = inks(
        &Inks {
            window: hex(0xfbe9f2),
            panel: hex(0xfff6fa),
            header: hex(0xfdeef5),
            border: hex(0xecc6da),
            text: hex(0x3a2236),
            muted: hex(0x74506c),
            accent: hex(0xc23c84),
            playhead: hex(0x2e8a7a),
            note: hex(0x9a5ac8),
        },
        true,
    );
    veil(&mut p, 0xd8, 0xe6);
    let mut theme = look(
        "Bubblegum",
        "Pastel pink, mint and lilac blobs that squash gently on the beat.",
        p,
        12.0,
    );
    ground(
        &mut theme,
        "builtin:bubblegum",
        [hex(0xff9ccc), hex(0x9de8d0), hex(0xfbe9f2), hex(0xc8a8f0)],
        1.0,
    );
    theme
}

/// A concert stage at night: teal and pink light over a dark stage, round
/// shapes and a rounded face. The shader, palette, shape and faces only —
/// any character art is the user's to add as a picture.
pub(super) fn stage() -> Theme {
    let teal = hex(0x39c5bb);
    let pink = hex(0xff4fa3);
    let mut p = inks(
        &Inks {
            window: hex(0x061214),
            panel: hex(0x0a1c1f),
            header: hex(0x0d2427),
            border: hex(0x1d4a4c),
            text: hex(0xeafcfa),
            muted: hex(0xa2d6d1),
            accent: teal,
            playhead: pink,
            note: teal,
        },
        false,
    );
    p.note_selected = hex(0x8ef0e6);
    p.selection = pink.with_alpha(0x5c);
    p.meter = teal;
    p.row_scale_root = mix(p.panel, pink, 0.16);
    veil(&mut p, 0xc8, 0xd8);
    let mut theme = look(
        "Stage",
        "A concert stage at night: teal and pink lights on a beat-lit floor.",
        p,
        12.0,
    );
    theme.font = FontTokens {
        family: "Rounded Mplus 1c".to_string(),
        size: 13.5,
        line_height: 1.35,
        display: Some("Mochiy Pop One".to_string()),
    };
    theme.fonts = vec![
        font(include_bytes!(
            "../../fonts/RoundedMplus1c-Medium-subset.ttf"
        )),
        font(include_bytes!(
            "../../fonts/MochiyPopOne-Regular-subset.ttf"
        )),
    ];
    ground(
        &mut theme,
        "builtin:stage",
        [teal, pink, hex(0x061214), hex(0xffffff)],
        1.0,
    );
    // The bar along the top lit teal to pink, added over the stage.
    theme.backdrops.transport = vec![Layer::Gradient(GradientLayer {
        stops: vec![(0.0, teal.with_alpha(0x38)), (1.0, pink.with_alpha(0x38))],
        angle: 0.0,
        radial: false,
        opacity: 1.0,
        blend: Blend::Add,
    })];
    theme
}

// ------------------------------------------------------------ retro ---

pub(super) fn neon_grid() -> Theme {
    let mut p = inks(
        &Inks {
            window: hex(0x0d0618),
            panel: hex(0x160b26),
            header: hex(0x1d0f31),
            border: hex(0x3e2463),
            text: hex(0xf4e9ff),
            muted: hex(0xb9a3d6),
            accent: hex(0xff3fb4),
            playhead: hex(0x3ff0ff),
            note: hex(0xc04ad8),
        },
        false,
    );
    veil(&mut p, 0xcc, 0xdc);
    let mut theme = look(
        "Neon Grid",
        "Synthwave: a banded sun over a neon floor that scrolls with the song.",
        p,
        0.0,
    );
    ground(
        &mut theme,
        "builtin:neon",
        [hex(0xff5ca8), hex(0x3ff0ff), hex(0x1a0a33), hex(0xffd36a)],
        1.0,
    );
    theme
}

pub(super) fn phosphor() -> Theme {
    let mut p = inks(
        &Inks {
            window: hex(0x030805),
            panel: hex(0x07120b),
            header: hex(0x0a1810),
            border: hex(0x1a4a2a),
            text: hex(0x7dfca0),
            muted: hex(0x4fb874),
            accent: hex(0x3df57a),
            playhead: hex(0xc8ffd8),
            note: hex(0x2fae5a),
        },
        false,
    );
    // One phosphor: the meter and the clip warning in its shades too, the
    // red kept for clipping alone.
    p.meter = hex(0x3df57a);
    p.key_white = hex(0x5fd884);
    veil(&mut p, 0xd8, 0xe4);
    let mut theme = look(
        "Phosphor",
        "A green CRT: scanlines, falling characters, an oscilloscope trace.",
        p,
        2.0,
    );
    theme.font.family = "monospace".to_string();
    ground(
        &mut theme,
        "builtin:phosphor",
        [hex(0x3df57a), hex(0x3df57a), hex(0x020604), hex(0x7dfca0)],
        1.0,
    );
    theme
}

// ---------------------------------------------------- high contrast ---

/// Black and white, thicker borders, larger text in Atkinson Hyperlegible
/// Next, and nothing behind any panel: WCAG 2.3.3 and Floptle's own choice
/// give a high-contrast look no animation, and fully opaque panels so
/// nothing moves behind text.
fn high_contrast_base(name: &str, description: &str, palette: Palette) -> Theme {
    let mut theme = look(name, description, palette, 4.0);
    theme.metrics = super::Metrics {
        border_width: 2.0,
        ..METRICS
    };
    theme.font = FontTokens {
        family: "Atkinson Hyperlegible Next".to_string(),
        size: 14.5,
        line_height: 1.35,
        display: None,
    };
    theme.fonts = vec![font(include_bytes!(
        "../../fonts/AtkinsonHyperlegibleNext.ttf"
    ))];
    theme.backdrops = Backdrops::default();
    theme
}

pub(super) fn high_contrast() -> Theme {
    let mut p = inks(
        &Inks {
            window: hex(0x000000),
            panel: hex(0x000000),
            header: hex(0x0d0d0d),
            border: hex(0xffffff),
            text: hex(0xffffff),
            muted: hex(0xd6d6d6),
            accent: hex(0xffd400),
            playhead: hex(0x00e5ff),
            note: hex(0x2f9bff),
        },
        false,
    );
    p.grid_line_sub = hex(0x262626);
    p.grid_line = hex(0x4a4a4a);
    p.grid_line_strong = hex(0x8a8a8a);
    p.selection = hex(0xffd400).with_alpha(0x60);
    p.note_selected = hex(0xffffff);
    p.key_white = hex(0xffffff);
    p.key_black = hex(0x000000);
    p.row_accidental = hex(0x141414);
    p.row_out_of_scale = hex(0x0a0a0a);
    p.row_scale_root = hex(0x332b00);
    p.meter = hex(0x00e676);
    p.meter_peak = hex(0xff3d3d);
    high_contrast_base(
        "High Contrast",
        "White on black, bold edges and larger text. Nothing moves.",
        p,
    )
}

pub(super) fn high_contrast_light() -> Theme {
    let mut p = inks(
        &Inks {
            window: hex(0xffffff),
            panel: hex(0xffffff),
            header: hex(0xf0f0f0),
            border: hex(0x000000),
            text: hex(0x000000),
            muted: hex(0x2e2e2e),
            accent: hex(0x0040c0),
            playhead: hex(0xc00060),
            note: hex(0x0050d0),
        },
        true,
    );
    p.grid_line_sub = hex(0xe0e0e0);
    p.grid_line = hex(0xb0b0b0);
    p.grid_line_strong = hex(0x6a6a6a);
    p.selection = hex(0x0040c0).with_alpha(0x48);
    p.note_selected = hex(0x001a66);
    p.key_white = hex(0xffffff);
    p.key_black = hex(0x000000);
    p.row_accidental = hex(0xf2f2f2);
    p.row_out_of_scale = hex(0xe6e6e6);
    p.row_scale_root = hex(0xdfe8ff);
    p.meter = hex(0x007a3d);
    p.meter_peak = hex(0xc00000);
    high_contrast_base(
        "High Contrast Light",
        "Black on white, bold edges and larger text. Nothing moves.",
        p,
    )
}
