//! Everybody else in a session, drawn where they are in the song: a pointer
//! in their colour with their name beside it, an arrow at the edge when the
//! place is off screen, their time marker as a thin line down the
//! arrangement, and the mixer strip they point at lit. Where each mark goes
//! is `canvas::person_marks`'s business; this only paints.

use vello::Scene;
use vello::kurbo::{Affine, BezPath, Point, Stroke};
use vello::peniko::Fill;

use super::{draw_text, fill_rect, fill_rect_rounded, stroke_rect_rounded};
use crate::canvas::{Marks, Spot, TimelineLayout, Toward, peer_colour};
use crate::layout::Rect;
use crate::text::Labels;
use crate::theme::{Color, Theme};

/// A person's colour, and the ink that reads on it.
fn colours(mark: &Marks) -> (Color, Color) {
    let [r, g, b, _] = peer_colour(mark.colour);
    let light = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b) > 150.0;
    let ink = if light {
        Color::rgb(0x14, 0x14, 0x18)
    } else {
        Color::rgb(0xff, 0xff, 0xff)
    };
    (Color::rgb(r, g, b), ink)
}

fn fill_path(scene: &mut Scene, path: &BezPath, colour: Color) {
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        colour.to_peniko(),
        None,
        path,
    );
}

fn stroke_path(scene: &mut Scene, path: &BezPath, width: f32, colour: Color) {
    scene.stroke(
        &Stroke::new(f64::from(width)),
        Affine::IDENTITY,
        colour.to_peniko(),
        None,
        path,
    );
}

/// A polygon through `points`, offset to (`x`, `y`) and scaled.
fn polygon(points: &[(f32, f32)], x: f32, y: f32, scale: f32) -> BezPath {
    let mut path = BezPath::new();
    for (i, (px, py)) in points.iter().enumerate() {
        let p = Point::new(f64::from(x + px * scale), f64::from(y + py * scale));
        if i == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    path.close_path();
    path
}

/// The pointer everyone knows, tip at the origin.
const POINTER: [(f32, f32); 7] = [
    (0.0, 0.0),
    (0.0, 15.0),
    (4.0, 11.5),
    (7.0, 17.5),
    (9.5, 16.5),
    (6.5, 10.5),
    (11.0, 10.5),
];

/// The name beside a mark, in a pill of the person's colour.
fn name_tag(
    scene: &mut Scene,
    labels: &Labels,
    name: &str,
    x: f32,
    y: f32,
    fill: Color,
    ink: Color,
) {
    let Some(shaped) = labels.get(name) else {
        return;
    };
    let pad = 4.0;
    let pill = Rect::new(x, y, shaped.width + pad * 2.0, shaped.height + 2.0);
    fill_rect_rounded(scene, pill, pill.height / 2.0, fill);
    draw_text(scene, shaped, x + pad, y + 1.0, ink);
}

pub(super) fn draw_people(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    people: &[Marks],
    timeline: Option<&TimelineLayout>,
) {
    let outline = theme.palette.window;
    for mark in people {
        let (fill, ink) = colours(mark);
        if let Some(strip) = mark.strip {
            fill_rect_rounded(
                scene,
                strip,
                theme.metrics.corner_radius,
                fill.with_alpha(0x22),
            );
            stroke_rect_rounded(scene, strip, theme.metrics.corner_radius, 2.0, fill);
            name_tag(
                scene,
                labels,
                &mark.name,
                strip.x + 2.0,
                strip.y + 2.0,
                fill,
                ink,
            );
        }
        if let (Some(x), Some(timeline)) = (mark.playhead, timeline) {
            let top = timeline.ruler.y;
            // Two whole pixels, solid: thin enough not to hide a clip edge,
            // and never smeared across three columns.
            let line = Rect::new(x.round() - 1.0, top, 2.0, timeline.grid.bottom() - top);
            fill_rect(scene, line, fill);
            // The name hangs off the top of the line, on the ruler.
            name_tag(scene, labels, &mark.name, x + 2.0, top + 1.0, fill, ink);
        }
        match mark.cursor {
            Some(Spot::Here { x, y }) => {
                let scale = if mark.followed { 1.5 } else { 1.0 };
                let arrow = polygon(&POINTER, x, y, scale);
                fill_path(scene, &arrow, fill);
                stroke_path(scene, &arrow, 1.0, outline);
                name_tag(
                    scene,
                    labels,
                    &mark.name,
                    x + 12.0 * scale,
                    y + 15.0 * scale,
                    fill,
                    ink,
                );
                // Something just said, in a bubble over the pointer.
                if let Some(said) = &mark.said
                    && let Some(shaped) = labels.get(said)
                {
                    let pad = 6.0;
                    let bubble = Rect::new(
                        x + 10.0,
                        y - shaped.height - pad * 2.0 - 4.0,
                        shaped.width + pad * 2.0,
                        shaped.height + pad,
                    );
                    fill_rect_rounded(scene, bubble, 6.0, theme.palette.panel);
                    stroke_rect_rounded(scene, bubble, 6.0, 1.5, fill);
                    draw_text(
                        scene,
                        shaped,
                        bubble.x + pad,
                        bubble.y + pad / 2.0,
                        theme.palette.text,
                    );
                }
            }
            Some(Spot::Off { x, y, toward }) => {
                let tip: [(f32, f32); 3] = match toward {
                    Toward::Left => [(-6.0, 0.0), (4.0, -6.0), (4.0, 6.0)],
                    Toward::Right => [(6.0, 0.0), (-4.0, -6.0), (-4.0, 6.0)],
                    Toward::Up => [(0.0, -6.0), (-6.0, 4.0), (6.0, 4.0)],
                    Toward::Down => [(0.0, 6.0), (-6.0, -4.0), (6.0, -4.0)],
                };
                let arrow = polygon(&tip, x, y, 1.0);
                fill_path(scene, &arrow, fill);
                stroke_path(scene, &arrow, 1.0, outline);
                let width = labels.get(&mark.name).map_or(0.0, |s| s.width + 8.0);
                // The name on the side away from the edge.
                let (tx, ty) = match toward {
                    Toward::Right => (x - 10.0 - width, y - 8.0),
                    Toward::Left => (x + 10.0, y - 8.0),
                    Toward::Up => (x - width / 2.0, y + 8.0),
                    Toward::Down => (x - width / 2.0, y - 26.0),
                };
                name_tag(scene, labels, &mark.name, tx, ty, fill, ink);
            }
            None => {}
        }
    }
}
