//! The bridge: Flopsynth's window as the inside of a ship.
//!
//! > *"improves the spacey scifi visual design of the flopsynth instrument to
//! > make it resemble a futuristic spaceship interior looking out through the
//! > window at the stars with a control panel ... all of the controls of the
//! > synth should be presented like racks of knobs like a control panel of
//! > the space ship ... actual space ship esque shaped design, sci fi
//! > materials ... an overal futuristic design thats not so flat and is more
//! > immersive and has more feedback."* — Ty, 2026-09-15
//!
//! Three things make a room out of a window, and this module draws each:
//!
//! - **The hull** ([`draw_hull`]): the ground under everything, dark plated
//!   metal with seams and rivets rather than a flat panel colour — brushed
//!   with fine lines, lit from above, and darker towards the floor.
//! - **The canopy** ([`draw_canopy`]): the bridge's window, a framed opening
//!   onto the sky ([`crate::sky`]) with struts and bolts, and the page tabs
//!   floating on it as a head-up display.
//! - **The consoles** ([`draw_console`]): each card as a module set into the
//!   hull — a bevelled recess with a nameplate, a status lamp in its family's
//!   ink that brightens while a control in it is held, and its picture on an
//!   inset screen ([`draw_screen`]).
//!
//! Everything is the theme's own colours, mixed: a light theme gets a pale
//! ship rather than a dark one pasted over it. A texture dropped into
//! `assets/flopsynth/skin/` is used where one is (see that folder's README);
//! without one, every surface here is procedural.

use vello::Scene;
use vello::kurbo::{Affine, BezPath, Circle, Ellipse, Point, Stroke};
use vello::peniko::{
    BlendMode, Blob, Brush, Extend, Fill, Gradient, ImageAlphaType, ImageBrush, ImageData,
    ImageFormat, ImageQuality, ImageSampler,
};

use super::{
    draw_text_clipped, fill_glow, fill_rect, fill_rect_rounded, fill_rect_vertical, lighten, mix,
    stroke_polyline, stroke_rect_rounded,
};
use crate::layout::Rect;
use crate::skin::Skin;
use crate::sky::{PlanetSprite, ShootingStar, StarSprite};
use crate::text::Labels;
use crate::theme::{Color, Theme};

/// One frame of the sky, ready to draw: the shaded nebula as an image and
/// the sprites over it. Built by the window from its [`crate::sky::SkyState`]
/// once a frame; `None` in the chrome draws a still sky.
#[derive(Debug, Clone)]
pub struct SkyFrame {
    /// The nebula, at a fraction of the canopy's size; scaled up bilinear.
    pub image: Option<ImageData>,
    pub stars: Vec<StarSprite>,
    pub shooting: Vec<ShootingStar>,
    pub planets: Vec<PlanetSprite>,
    /// The waveform ribbon, one point per column.
    pub aurora: Vec<(f32, f32)>,
    /// How loud it is, 0..1: what the lamps and the aurora's glow follow.
    pub level: f32,
}

impl SkyFrame {
    /// An image for `draw_image`, from a shaded [`crate::sky::SkyImage`].
    pub fn image_of(sky: &crate::sky::SkyImage) -> Option<ImageData> {
        if sky.width == 0 || sky.height == 0 {
            return None;
        }
        Some(ImageData {
            data: Blob::new(std::sync::Arc::new(sky.rgba.clone())),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width: sky.width,
            height: sky.height,
        })
    }
}

/// How thick the canopy's frame is.
pub const CANOPY_FRAME: f32 = 7.0;
/// The canopy's corners: rounder at the top, where a windshield curves into
/// the hull, than at the bottom, where it meets the dash.
const CANOPY_RADIUS_TOP: f32 = 26.0;
const CANOPY_RADIUS_BOTTOM: f32 = 8.0;

/// Tiles `image` across `rect` at its own size, at `alpha`.
fn tile(scene: &mut Scene, image: &ImageData, rect: Rect, alpha: f32) {
    if rect.is_empty() || image.width == 0 || image.height == 0 {
        return;
    }
    let brush = ImageBrush {
        image: image.clone(),
        sampler: ImageSampler::new()
            .with_extend(Extend::Repeat)
            .with_quality(ImageQuality::Medium)
            .with_alpha(alpha),
    };
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        &brush,
        Some(Affine::translate((rect.x as f64, rect.y as f64))),
        &vello::kurbo::Rect::new(
            rect.x as f64,
            rect.y as f64,
            rect.right() as f64,
            rect.bottom() as f64,
        ),
    );
}

/// Stretches `image` over `rect`, at `alpha`.
fn stretch(scene: &mut Scene, image: &ImageData, rect: Rect, alpha: f32) {
    if rect.is_empty() || image.width == 0 || image.height == 0 {
        return;
    }
    let brush = ImageBrush {
        image: image.clone(),
        sampler: ImageSampler::new()
            .with_quality(ImageQuality::Medium)
            .with_alpha(alpha),
    };
    scene.draw_image(
        &brush,
        Affine::translate((rect.x as f64, rect.y as f64))
            * Affine::scale_non_uniform(
                rect.width as f64 / image.width as f64,
                rect.height as f64 / image.height as f64,
            ),
    );
}

/// A colour moved `amount` of the way towards black.
fn darken(colour: Color, amount: f32) -> Color {
    let mix = |c: u8| (f32::from(c) * (1.0 - amount)) as u8;
    Color([
        mix(colour.0[0]),
        mix(colour.0[1]),
        mix(colour.0[2]),
        colour.0[3],
    ])
}

/// The hull: the ground of the whole window, drawn before anything is set
/// into it.
///
/// Plated metal: a base graded from lit at the top to dark at the floor,
/// brushed with fine horizontal lines, divided into plates by seams with a
/// bevel — a light line over a dark one, which is what an edge lit from
/// above looks like — and riveted where the plates meet. The dashboard's lip
/// runs under the canopy.
pub(super) fn draw_hull(
    scene: &mut Scene,
    theme: &Theme,
    ground: Rect,
    dash_y: f32,
    skin: Option<&Skin>,
) {
    let p = &theme.palette;
    if ground.is_empty() {
        return;
    }
    let base = mix(p.window, p.panel, 0.35);
    fill_rect_vertical(scene, ground, 0.0, lighten(base, 0.06), darken(base, 0.35));
    // A plate texture from the skin folder, tiled under everything and
    // tinted by the grade above through its own alpha.
    if let Some(hull) = skin.and_then(|s| s.hull.as_ref()) {
        tile(scene, hull, ground, 0.55);
    }
    // Brushed: every third row a hair lighter. Cheap, and what makes the
    // surface read as metal rather than paint.
    let brush = p.text.with_alpha(0x05);
    let mut y = ground.y;
    while y < ground.bottom() {
        fill_rect(scene, Rect::new(ground.x, y, ground.width, 1.0), brush);
        y += 3.0;
    }
    // Plates: two vertical seams at the thirds of the width and the dash
    // seam across, each a bevel.
    let seam_dark = p.window.with_alpha(0xb0);
    let seam_light = p.text.with_alpha(0x1c);
    let bevel_h = |scene: &mut Scene, y: f32| {
        fill_rect(scene, Rect::new(ground.x, y, ground.width, 1.0), seam_dark);
        fill_rect(
            scene,
            Rect::new(ground.x, y + 1.0, ground.width, 1.0),
            seam_light,
        );
    };
    let bevel_v = |scene: &mut Scene, x: f32| {
        fill_rect(
            scene,
            Rect::new(x, dash_y, 1.0, ground.bottom() - dash_y),
            seam_dark,
        );
        fill_rect(
            scene,
            Rect::new(x + 1.0, dash_y, 1.0, ground.bottom() - dash_y),
            seam_light,
        );
    };
    if dash_y > ground.y && dash_y < ground.bottom() {
        bevel_h(scene, dash_y);
    }
    for third in [1.0, 2.0] {
        bevel_v(scene, (ground.x + ground.width * third / 3.0).round());
    }
    // Rivets along the dash seam: a dark ring with a lit dome.
    if dash_y > ground.y && dash_y < ground.bottom() {
        let mut x = ground.x + 14.0;
        while x < ground.right() - 8.0 {
            rivet(scene, theme, (x, dash_y + 8.0), 2.2);
            x += 48.0;
        }
    }
}

/// A rivet: a dark ring with a lit dome.
fn rivet(scene: &mut Scene, theme: &Theme, centre: (f32, f32), radius: f32) {
    let p = &theme.palette;
    let (cx, cy) = (centre.0 as f64, centre.1 as f64);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        p.window.with_alpha(0xc0).to_peniko(),
        None,
        &Circle::new((cx, cy), (radius + 1.0) as f64),
    );
    let dome = Gradient::new_radial(Point::new(cx - 0.6, cy - 0.7), radius * 1.4).with_stops([
        (0.0, p.text.with_alpha(0x60).to_peniko()),
        (1.0, p.panel_header.with_alpha(0x80).to_peniko()),
    ]);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        &Brush::Gradient(dome),
        None,
        &Circle::new((cx, cy), radius as f64),
    );
}

/// The windshield's shape: rounder at the top than at the bottom.
fn canopy_path(rect: Rect) -> BezPath {
    let (x0, y0, x1, y1) = (
        rect.x as f64,
        rect.y as f64,
        rect.right() as f64,
        rect.bottom() as f64,
    );
    let rt = (CANOPY_RADIUS_TOP as f64)
        .min((x1 - x0) / 2.0)
        .min((y1 - y0) / 2.0);
    let rb = (CANOPY_RADIUS_BOTTOM as f64)
        .min((x1 - x0) / 2.0)
        .min((y1 - y0) / 2.0);
    let k = 0.5523;
    let mut path = BezPath::new();
    path.move_to((x0 + rt, y0));
    path.line_to((x1 - rt, y0));
    path.curve_to(
        (x1 - rt + rt * k, y0),
        (x1, y0 + rt - rt * k),
        (x1, y0 + rt),
    );
    path.line_to((x1, y1 - rb));
    path.curve_to(
        (x1, y1 - rb + rb * k),
        (x1 - rb + rb * k, y1),
        (x1 - rb, y1),
    );
    path.line_to((x0 + rb, y1));
    path.curve_to(
        (x0 + rb - rb * k, y1),
        (x0, y1 - rb + rb * k),
        (x0, y1 - rb),
    );
    path.line_to((x0, y0 + rt));
    path.curve_to(
        (x0, y0 + rt - rt * k),
        (x0 + rt - rt * k, y0),
        (x0 + rt, y0),
    );
    path.close_path();
    path
}

/// The canopy: the sky through the bridge's window, in `opening`, with its
/// frame around it.
///
/// The sky is clipped to the windshield's shape and drawn in layers — the
/// nebula image, the planets, the star field, the stars in flight, and the
/// aurora — then the frame is set over it: a thick dark rim with a bevel, a
/// bolt at each corner, and a strut down either side. With no frame of sky
/// to draw (`sky` is `None`: the headless shot, a window before its first
/// tick) a still sky is drawn from the theme alone, so the picture is never
/// a hole.
pub(super) fn draw_canopy(
    scene: &mut Scene,
    theme: &Theme,
    opening: Rect,
    sky: Option<&SkyFrame>,
    skin: Option<&Skin>,
) {
    let p = &theme.palette;
    if opening.is_empty() {
        return;
    }
    let window = canopy_path(opening);
    scene.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        1.0,
        Affine::IDENTITY,
        &window,
    );

    // Deep space, then the nebula over it.
    fill_rect_vertical(
        scene,
        opening,
        0.0,
        darken(p.window, 0.6),
        mix(darken(p.window, 0.3), p.modulation, 0.12),
    );
    match sky.and_then(|s| s.image.as_ref()) {
        Some(image) => {
            let sx = opening.width as f64 / image.width.max(1) as f64;
            let sy = opening.height as f64 / image.height.max(1) as f64;
            let brush = ImageBrush {
                image: image.clone(),
                sampler: ImageSampler::new().with_quality(ImageQuality::Medium),
            };
            scene.draw_image(
                &brush,
                Affine::translate((opening.x as f64, opening.y as f64))
                    * Affine::scale_non_uniform(sx, sy),
            );
        }
        None => {
            // A still sky: two nebulae and a scatter of stars, the theme's
            // colours, the same picture the old ground drew.
            fill_glow(
                scene,
                (
                    opening.x + opening.width * 0.22,
                    opening.y + opening.height * 0.3,
                ),
                opening.width * 0.4,
                p.accent,
                0x38,
            );
            fill_glow(
                scene,
                (
                    opening.right() - opening.width * 0.2,
                    opening.bottom() - opening.height * 0.2,
                ),
                opening.width * 0.35,
                p.modulation,
                0x30,
            );
            let mut seed: u32 = 0x9e37_79b9;
            for _ in 0..110 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let x = opening.x + opening.width * ((seed >> 8) & 0xffff) as f32 / 65_536.0;
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let y = opening.y + opening.height * ((seed >> 8) & 0xffff) as f32 / 65_536.0;
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let bright = 0x40 + ((seed >> 8) & 0x7f) as u8;
                let size = if (seed >> 20) & 0x7 == 0 { 2.0 } else { 1.0 };
                fill_rect(
                    scene,
                    Rect::new(x, y, size, size),
                    p.text.with_alpha(bright),
                );
            }
        }
    }

    if let Some(sky) = sky {
        for planet in &sky.planets {
            draw_planet(scene, theme, planet);
        }
        for star in &sky.stars {
            let ink = mix(
                lighten(p.accent, 0.6),
                lighten(p.playhead, 0.5),
                star.warmth,
            );
            let alpha = (star.brightness * 255.0) as u8;
            if star.size > 1.8 {
                fill_glow(scene, (star.x, star.y), star.size * 3.0, ink, alpha / 3);
            }
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                ink.with_alpha(alpha).to_peniko(),
                None,
                &Circle::new((star.x as f64, star.y as f64), (star.size * 0.5) as f64),
            );
        }
        for star in &sky.shooting {
            let alpha = (star.life * 220.0) as u8;
            let ink = lighten(p.text, 0.3);
            let mut path = BezPath::new();
            path.move_to((star.from.0 as f64, star.from.1 as f64));
            path.line_to((star.to.0 as f64, star.to.1 as f64));
            scene.stroke(
                &Stroke::new(1.2),
                Affine::IDENTITY,
                ink.with_alpha(alpha).to_peniko(),
                None,
                &path,
            );
            fill_glow(scene, star.to, 6.0, ink, alpha / 2);
        }
        // The aurora: the waveform as a ribbon of light, wide and faint under
        // thin and bright, in the accent leaning to violet with the level.
        if sky.aurora.len() >= 2 {
            let ink = mix(p.accent, p.modulation, sky.level * 0.6);
            let glow = (0x30 + (sky.level * 0x60 as f32) as u8).min(0xa0);
            stroke_polyline(scene, &sky.aurora, opening, 14.0, ink.with_alpha(glow / 3));
            stroke_polyline(scene, &sky.aurora, opening, 5.0, ink.with_alpha(glow));
            stroke_polyline(
                scene,
                &sky.aurora,
                opening,
                1.2,
                lighten(ink, 0.5).with_alpha(0xd0),
            );
        }
    }
    // The windshield's own surface from the skin folder — scratches, grime,
    // a flare — stretched over the opening, alpha and all.
    if let Some(glass) = skin.and_then(|s| s.glass.as_ref()) {
        stretch(scene, glass, opening, 1.0);
    }
    // Glass: a faint sheen across the top, so the opening reads as a pane
    // and not a hole.
    fill_rect_vertical(
        scene,
        Rect::new(opening.x, opening.y, opening.width, opening.height * 0.35),
        0.0,
        p.text.with_alpha(0x14),
        p.text.with_alpha(0x00),
    );
    scene.pop_layer();

    // The frame: a rim of hull around the opening, bevelled, with a bolt at
    // each corner and a strut down each side.
    let rim = mix(p.panel_header, p.window, 0.3);
    scene.stroke(
        &Stroke::new(f64::from(CANOPY_FRAME)),
        Affine::IDENTITY,
        rim.to_peniko(),
        None,
        &window,
    );
    scene.stroke(
        &Stroke::new(1.0),
        Affine::translate((0.0, -f64::from(CANOPY_FRAME) / 2.0 + 0.5)),
        p.text.with_alpha(0x30).to_peniko(),
        None,
        &window,
    );
    scene.stroke(
        &Stroke::new(1.5),
        Affine::IDENTITY,
        darken(p.window, 0.5).with_alpha(0xc0).to_peniko(),
        None,
        &window,
    );
    for (dx, dy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
        let x = if dx > 0.0 {
            opening.x + 18.0
        } else {
            opening.right() - 18.0
        };
        let y = if dy > 0.0 {
            opening.y + 14.0
        } else {
            opening.bottom() - 12.0
        };
        rivet(scene, theme, (x, y), 2.4);
    }
    // Struts: a pair of angled ribs near each side, the way a cockpit's
    // frame is braced.
    let strut = rim.with_alpha(0xd0);
    for side in [0.0f32, 1.0] {
        let inset = opening.width * 0.09;
        let x_top = if side == 0.0 {
            opening.x + inset
        } else {
            opening.right() - inset
        };
        let x_bottom = if side == 0.0 {
            opening.x + inset * 0.4
        } else {
            opening.right() - inset * 0.4
        };
        let mut path = BezPath::new();
        path.move_to((x_top as f64, opening.y as f64));
        path.line_to((x_bottom as f64, opening.bottom() as f64));
        scene.stroke(
            &Stroke::new(4.0),
            Affine::IDENTITY,
            strut.to_peniko(),
            None,
            &path,
        );
        scene.stroke(
            &Stroke::new(1.0),
            Affine::translate((if side == 0.0 { -1.5 } else { 1.5 }, 0.0)),
            p.text.with_alpha(0x28).to_peniko(),
            None,
            &path,
        );
    }
}

/// A planet: a disc lit from the upper left, a terminator into shadow, and a
/// ring when it has one.
fn draw_planet(scene: &mut Scene, theme: &Theme, planet: &PlanetSprite) {
    let p = &theme.palette;
    let ink = mix(p.accent, p.modulation, planet.ink);
    let (cx, cy, r) = (planet.x as f64, planet.y as f64, planet.radius as f64);
    if r < 1.0 {
        return;
    }
    // The ring's far half, behind the disc.
    if planet.ring > 0.0 {
        let ring = Ellipse::new((cx, cy), (r * 1.9, r * 1.9 * f64::from(planet.ring)), 0.0);
        scene.stroke(
            &Stroke::new(r * 0.16),
            Affine::IDENTITY,
            lighten(ink, 0.3).with_alpha(0x70).to_peniko(),
            None,
            &ring,
        );
    }
    fill_glow(
        scene,
        (planet.x, planet.y),
        planet.radius * 1.8,
        ink,
        (0x28 as f32 * (0.5 + planet.lit)) as u8,
    );
    let day = Gradient::new_radial(Point::new(cx - r * 0.45, cy - r * 0.45), (r * 1.5) as f32)
        .with_stops([
            (0.0, lighten(ink, 0.55 * planet.lit + 0.1).to_peniko()),
            (0.55, ink.to_peniko()),
            (1.0, darken(ink, 0.75).to_peniko()),
        ]);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        &Brush::Gradient(day),
        None,
        &Circle::new((cx, cy), r),
    );
    // The ring's near half, over the disc: clipped to the lower half.
    if planet.ring > 0.0 {
        let clip = vello::kurbo::Rect::new(cx - r * 2.2, cy, cx + r * 2.2, cy + r * 2.2);
        scene.push_layer(
            Fill::NonZero,
            BlendMode::default(),
            1.0,
            Affine::IDENTITY,
            &clip,
        );
        let ring = Ellipse::new((cx, cy), (r * 1.9, r * 1.9 * f64::from(planet.ring)), 0.0);
        scene.stroke(
            &Stroke::new(r * 0.16),
            Affine::IDENTITY,
            lighten(ink, 0.4).with_alpha(0xa0).to_peniko(),
            None,
            &ring,
        );
        scene.pop_layer();
    }
}

/// The page tabs as a head-up display floating on the sky: outlined chips,
/// the page you are on lit.
pub(super) fn draw_hud_tab(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    rect: Rect,
    label: &str,
    here: bool,
) {
    let p = &theme.palette;
    if rect.is_empty() {
        return;
    }
    let radius = 4.0;
    if here {
        fill_glow(
            scene,
            (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
            rect.width * 0.6,
            p.accent,
            0x40,
        );
        fill_rect_rounded(scene, rect, radius, p.accent.with_alpha(0x50));
        stroke_rect_rounded(scene, rect, radius, 1.0, lighten(p.accent, 0.3));
        // The HUD's bracket marks at the ends.
        for x in [rect.x + 3.0, rect.right() - 4.0] {
            fill_rect(
                scene,
                Rect::new(x, rect.y + 4.0, 1.0, rect.height - 8.0),
                lighten(p.accent, 0.5),
            );
        }
    } else {
        fill_rect_rounded(scene, rect, radius, p.window.with_alpha(0x70));
        stroke_rect_rounded(scene, rect, radius, 1.0, p.accent.with_alpha(0x50));
    }
    if let Some(text) = labels.get(label) {
        draw_text_clipped(
            scene,
            text,
            rect,
            rect.x + ((rect.width - text.width) / 2.0).max(2.0),
            rect.y + (rect.height - text.height) / 2.0,
            if here { lighten(p.text, 0.2) } else { p.accent },
        );
    }
}

/// One console: a card as a module set into the hull.
///
/// A bevelled recess — a dark line along the top and left where the hull's
/// edge shadows it, a light one along the bottom and right where the edge
/// catches the light — filled a shade darker than the hull, with a nameplate
/// across the top carrying the name and a status lamp in the family's ink.
/// The lamp is brighter while a control in the console is held (`hot`),
/// which is the feedback a rack of hardware gives: the module you are
/// working on is the one whose light is on.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_console(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    frame: Rect,
    header: Rect,
    name: &str,
    ink: Color,
    hot: bool,
    skin: Option<&Skin>,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    if frame.is_empty() {
        return;
    }
    let radius = m.corner_radius + 1.0;
    let base = mix(p.panel, p.window, 0.5);
    // The recess's shadow, a pixel out and down, then the recess.
    fill_rect_rounded(
        scene,
        Rect::new(
            frame.x - 1.0,
            frame.y - 1.0,
            frame.width + 2.0,
            frame.height + 3.0,
        ),
        radius + 1.0,
        darken(p.window, 0.5).with_alpha(0xa0),
    );
    fill_rect_vertical(
        scene,
        frame,
        radius,
        darken(base, 0.08).with_alpha(0xf4),
        darken(base, 0.28).with_alpha(0xf4),
    );
    if let Some(face) = skin.and_then(|s| s.console.as_ref()) {
        scene.push_layer(
            Fill::NonZero,
            BlendMode::default(),
            1.0,
            Affine::IDENTITY,
            &super::rounded(frame, radius),
        );
        tile(scene, face, frame, 0.6);
        scene.pop_layer();
    }
    // The bevel: dark on the lit-from-above edges that face away from the
    // light, light on the ones that face it.
    fill_rect(
        scene,
        Rect::new(
            frame.x + radius,
            frame.y + 1.0,
            (frame.width - radius * 2.0).max(0.0),
            1.0,
        ),
        darken(p.window, 0.6).with_alpha(0xa0),
    );
    fill_rect(
        scene,
        Rect::new(
            frame.x + 1.0,
            frame.y + radius,
            1.0,
            (frame.height - radius * 2.0).max(0.0),
        ),
        darken(p.window, 0.6).with_alpha(0x80),
    );
    fill_rect(
        scene,
        Rect::new(
            frame.x + radius,
            frame.bottom() - 2.0,
            (frame.width - radius * 2.0).max(0.0),
            1.0,
        ),
        p.text.with_alpha(0x22),
    );
    fill_rect(
        scene,
        Rect::new(
            frame.right() - 2.0,
            frame.y + radius,
            1.0,
            (frame.height - radius * 2.0).max(0.0),
        ),
        p.text.with_alpha(0x18),
    );
    stroke_rect_rounded(scene, frame, radius, 1.0, p.border.with_alpha(0xc0));

    // The nameplate: a strip a shade lighter, engraved with the name, the
    // lamp at its left end and a rule under it in the family's ink.
    if !header.is_empty() {
        fill_rect_vertical(
            scene,
            Rect::new(
                header.x + 2.0,
                header.y + 2.0,
                (header.width - 4.0).max(0.0),
                header.height - 2.0,
            ),
            radius - 1.0,
            lighten(base, 0.10),
            base,
        );
        let lamp = (header.x + 9.0, header.y + header.height / 2.0);
        fill_glow(
            scene,
            lamp,
            if hot { 14.0 } else { 8.0 },
            ink,
            if hot { 0xa0 } else { 0x50 },
        );
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            if hot { lighten(ink, 0.5) } else { ink }.to_peniko(),
            None,
            &Circle::new((lamp.0 as f64, lamp.1 as f64), 2.6),
        );
        // The family's rule under the name: faint across, solid at its
        // left end — the mark every card has carried since the first build,
        // and the one the headless shot samples.
        fill_rect(
            scene,
            Rect::new(
                header.x + 6.0,
                header.bottom() - 1.0,
                (header.width - 12.0).max(0.0),
                1.0,
            ),
            ink.with_alpha(if hot { 0xc0 } else { 0x70 }),
        );
        fill_rect(
            scene,
            Rect::new(
                header.x + 6.0,
                header.bottom() - 2.0,
                28.0_f32.min(header.width),
                2.0,
            ),
            if hot { lighten(ink, 0.3) } else { ink },
        );
        if let Some(text) = labels.get(name) {
            // Engraved: a dark copy a pixel down, the text over it.
            draw_text_clipped(
                scene,
                text,
                header,
                header.x + 18.0,
                header.y + (header.height - text.height) / 2.0 + 1.0,
                darken(p.window, 0.5).with_alpha(0x90),
            );
            draw_text_clipped(
                scene,
                text,
                header,
                header.x + 18.0,
                header.y + (header.height - text.height) / 2.0,
                if hot { lighten(p.text, 0.2) } else { p.text },
            );
        }
    }
}

/// A screen: the well a console's picture is drawn in, as an inset display
/// with a faint raster and a glow in the family's ink.
pub(super) fn draw_screen(scene: &mut Scene, theme: &Theme, rect: Rect, ink: Color) {
    let p = &theme.palette;
    let m = &theme.metrics;
    if rect.is_empty() {
        return;
    }
    // The bezel's shadow, then the glass.
    fill_rect_rounded(
        scene,
        Rect::new(
            rect.x - 1.0,
            rect.y - 1.0,
            rect.width + 2.0,
            rect.height + 2.0,
        ),
        m.corner_radius + 1.0,
        darken(p.window, 0.6).with_alpha(0xc0),
    );
    fill_rect_vertical(
        scene,
        rect,
        m.corner_radius,
        darken(p.window, 0.55),
        darken(mix(p.window, ink, 0.12), 0.35),
    );
    // The raster: every other row a hair darker, which is what makes glass
    // read as a display and not as a hole.
    let inner = rect.inset(2.0);
    let mut y = inner.y;
    while y < inner.bottom() {
        fill_rect(
            scene,
            Rect::new(inner.x, y, inner.width, 1.0),
            p.window.with_alpha(0x30),
        );
        y += 2.0;
    }
    stroke_rect_rounded(scene, rect, m.corner_radius, 1.0, ink.with_alpha(0x60));
    // A glint along the top edge of the glass.
    fill_rect(
        scene,
        Rect::new(rect.x + 4.0, rect.y + 1.0, (rect.width - 8.0).max(0.0), 1.0),
        p.text.with_alpha(0x1c),
    );
}

/// The tick marks round a knob, like the scale printed on a panel.
pub(super) fn draw_knob_ticks(scene: &mut Scene, theme: &Theme, centre: (f32, f32), radius: f32) {
    let p = &theme.palette;
    let ink = p.text.with_alpha(0x38);
    for i in 0..11 {
        let t = i as f32 / 10.0;
        let angle = (-0.75 + 1.5 * t) * std::f32::consts::PI;
        let (s, c) = angle.sin_cos();
        let inner = radius + 3.0;
        let outer = radius + if i % 5 == 0 { 6.0 } else { 4.5 };
        let mut path = BezPath::new();
        path.move_to(((centre.0 + inner * s) as f64, (centre.1 - inner * c) as f64));
        path.line_to(((centre.0 + outer * s) as f64, (centre.1 - outer * c) as f64));
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            ink.to_peniko(),
            None,
            &path,
        );
    }
}
