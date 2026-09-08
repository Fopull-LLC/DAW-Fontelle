//! The icon set, as geometry.
//!
//! Reported from using the window: *"I want the app to have more icons instead
//! of just text"*, and *"I want the cursor icons to actually represent the
//! action better."* Both are this file.
//!
//! # Why the icons are drawn rather than loaded
//!
//! vello is a path renderer, so an icon that **is** a path costs nothing extra
//! to draw and is crisp at any scale — a 2× display and a 1× one get the same
//! shape rather than the same pixels stretched. It takes the theme's own ink,
//! so a light theme does not need a second set. It ships no files, packs no
//! atlas, and tracks no licence. And the same shapes rasterise into the **mouse
//! cursor** ([`rasterise`]), so the pencil on the toolbar and the pencil under
//! the pointer are one definition rather than two that drift.
//!
//! # The shape vocabulary is deliberately tiny
//!
//! Polylines, filled polygons and circles. Every icon a DAW needs is one of
//! those three or a handful of them together, and a vocabulary with curves in
//! it would need a curve editor to author against. Keeping it to points means
//! the whole set is checkable by arithmetic: nothing empty, nothing outside its
//! box, nothing too small to read — see `tests/icons.rs`.
//!
//! Everything is in a **unit box**: x and y both run 0 to 1, y downwards, the
//! same way the layout's rectangles do. The renderer maps that onto whatever
//! rectangle it was given.

/// One piece of an icon.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// A stroked polyline. `closed` joins the last point back to the first.
    Line {
        points: Vec<(f32, f32)>,
        closed: bool,
    },
    /// A filled polygon.
    Poly(Vec<(f32, f32)>),
    Circle {
        at: (f32, f32),
        r: f32,
        filled: bool,
    },
}

impl Shape {
    fn line(points: &[(f32, f32)]) -> Self {
        Self::Line {
            points: points.to_vec(),
            closed: false,
        }
    }

    fn closed(points: &[(f32, f32)]) -> Self {
        Self::Line {
            points: points.to_vec(),
            closed: true,
        }
    }

    fn poly(points: &[(f32, f32)]) -> Self {
        Self::Poly(points.to_vec())
    }

    /// A stroked rectangle, which is half the icons here.
    fn rect(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self::closed(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)])
    }
}

/// Every glyph the chrome draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    // --- the transport ---
    Play,
    Stop,
    Record,
    Loop,
    Metronome,
    // --- the roll's tools ---
    Pencil,
    Brush,
    Marquee,
    Eraser,
    // --- editing ---
    Cut,
    Copy,
    Paste,
    /// Copies of a clip, laid after it. Not [`Icon::Loop`], which is one clip
    /// coming round again — the two icons are as different as the two features.
    Repeat,
    Trash,
    // --- switches and read-outs ---
    Mute,
    Solo,
    /// Snap.
    Magnet,
    /// The onion skin.
    Ghost,
    /// The property lane, and the mixer tab.
    Sliders,
    /// The piano-roll tab.
    Piano,
    /// Where a channel's audio goes.
    Route,
    // --- navigation ---
    ZoomIn,
    ZoomOut,
    ArrowUp,
    ArrowDown,
    Plus,
    Minus,
    Chevron,
    // --- files ---
    Folder,
    NewFile,
    /// An EQ curve: a flat line with a bell in it. What the effect tab is
    /// for, and legible at sixteen pixels because the bump is the whole shape.
    Curve,
    /// The **cut tool**: a blade leaning across the line it makes.
    ///
    /// Not [`Icon::Cut`], which is a pair of scissors and already means "cut to
    /// the clipboard" on the same toolbar. Cutting a clip in two and taking it
    /// away are two different things and cannot share a picture.
    Blade,
    /// A favourite that is not one yet: the hollow star on a menu row, which
    /// a press lights.
    Star,
    /// And one that is. Two icons rather than one drawn two ways, because
    /// the filled star is what a favourite *looks like* wherever it is drawn
    /// and the cursor rasteriser only knows icons.
    StarFilled,
}

/// Every icon, for a test that has to check all of them.
pub const EVERY_ICON: [Icon; 34] = [
    Icon::Play,
    Icon::Stop,
    Icon::Record,
    Icon::Loop,
    Icon::Metronome,
    Icon::Pencil,
    Icon::Brush,
    Icon::Marquee,
    Icon::Eraser,
    Icon::Cut,
    Icon::Copy,
    Icon::Paste,
    Icon::Repeat,
    Icon::Trash,
    Icon::Mute,
    Icon::Solo,
    Icon::Magnet,
    Icon::Ghost,
    Icon::Sliders,
    Icon::Piano,
    Icon::Route,
    Icon::ZoomIn,
    Icon::ZoomOut,
    Icon::ArrowUp,
    Icon::ArrowDown,
    Icon::Plus,
    Icon::Minus,
    Icon::Chevron,
    Icon::Folder,
    Icon::NewFile,
    Icon::Curve,
    Icon::Blade,
    Icon::Star,
    Icon::StarFilled,
];

/// What `icon` is made of, in the unit box.
pub fn shapes(icon: Icon) -> Vec<Shape> {
    match icon {
        Icon::Curve => vec![Shape::line(&[
            (0.08, 0.62),
            (0.26, 0.62),
            (0.36, 0.60),
            (0.44, 0.34),
            (0.52, 0.24),
            (0.60, 0.34),
            (0.68, 0.60),
            (0.78, 0.62),
            (0.92, 0.62),
        ])],
        Icon::Play => vec![Shape::poly(&[(0.22, 0.10), (0.88, 0.50), (0.22, 0.90)])],
        Icon::Stop => vec![Shape::poly(&[
            (0.18, 0.18),
            (0.82, 0.18),
            (0.82, 0.82),
            (0.18, 0.82),
        ])],
        Icon::Record => vec![Shape::Circle {
            at: (0.50, 0.50),
            r: 0.33,
            filled: true,
        }],
        // A rectangle open at its top right, with the arrowhead that closes it
        // — the loop arrow everything from a tape machine to a browser uses.
        Icon::Loop => vec![
            Shape::line(&[
                (0.62, 0.16),
                (0.88, 0.16),
                (0.88, 0.84),
                (0.12, 0.84),
                (0.12, 0.16),
                (0.38, 0.16),
            ]),
            Shape::poly(&[(0.34, 0.02), (0.34, 0.30), (0.58, 0.16)]),
        ],
        // A body that tapers to a point, and the pendulum across it.
        Icon::Metronome => vec![
            Shape::closed(&[(0.42, 0.10), (0.58, 0.10), (0.86, 0.88), (0.14, 0.88)]),
            Shape::line(&[(0.50, 0.84), (0.72, 0.24)]),
            Shape::line(&[(0.24, 0.62), (0.76, 0.62)]),
        ],
        // A slab with a point on it: the body is a rotated rectangle and the
        // tip is the triangle that finishes it.
        Icon::Pencil => vec![
            Shape::poly(&[(0.30, 0.86), (0.20, 0.72), (0.72, 0.14), (0.86, 0.26)]),
            Shape::poly(&[(0.10, 0.92), (0.20, 0.72), (0.30, 0.86)]),
        ],
        // A handle, and the loaded head that paints. The head is deliberately
        // chunky: at twenty-six pixels a thin one reads as a second line and
        // the whole glyph becomes "a diagonal", which is what the pencil
        // beside it already is.
        Icon::Brush => vec![
            Shape::line(&[(0.90, 0.10), (0.52, 0.46)]),
            Shape::poly(&[(0.56, 0.34), (0.74, 0.52), (0.42, 0.80), (0.24, 0.62)]),
            Shape::line(&[(0.24, 0.66), (0.08, 0.92)]),
        ],
        // Four corners rather than a box: a marquee is a selection you are
        // *making*, and the broken outline is how every tool says so.
        Icon::Marquee => vec![
            Shape::line(&[(0.12, 0.34), (0.12, 0.14), (0.34, 0.14)]),
            Shape::line(&[(0.66, 0.14), (0.88, 0.14), (0.88, 0.34)]),
            Shape::line(&[(0.88, 0.66), (0.88, 0.86), (0.66, 0.86)]),
            Shape::line(&[(0.34, 0.86), (0.12, 0.86), (0.12, 0.66)]),
        ],
        // A block held at an angle, with the worn end shaded off.
        Icon::Eraser => vec![
            Shape::closed(&[(0.10, 0.70), (0.56, 0.12), (0.90, 0.38), (0.44, 0.92)]),
            Shape::line(&[(0.30, 0.44), (0.68, 0.72)]),
        ],
        // Two blades crossing, and the two finger loops under them.
        // The line it leaves, and the blade that made it. The blade is filled
        // so it reads as an object rather than as a second stroke.
        // Ten points round a circle, alternating the tip radius and the notch
        // radius — the same star both ways, stroked or filled.
        Icon::Star => vec![Shape::closed(&[
            (0.50, 0.07),
            (0.62, 0.38),
            (0.95, 0.39),
            (0.69, 0.60),
            (0.78, 0.92),
            (0.50, 0.74),
            (0.22, 0.92),
            (0.31, 0.60),
            (0.05, 0.39),
            (0.38, 0.38),
        ])],
        Icon::StarFilled => vec![Shape::poly(&[
            (0.50, 0.07),
            (0.62, 0.38),
            (0.95, 0.39),
            (0.69, 0.60),
            (0.78, 0.92),
            (0.50, 0.74),
            (0.22, 0.92),
            (0.31, 0.60),
            (0.05, 0.39),
            (0.38, 0.38),
        ])],
        Icon::Blade => vec![
            Shape::line(&[(0.50, 0.04), (0.50, 0.96)]),
            Shape::poly(&[(0.14, 0.74), (0.60, 0.10), (0.74, 0.22), (0.28, 0.86)]),
        ],
        Icon::Cut => vec![
            Shape::line(&[(0.20, 0.10), (0.70, 0.66)]),
            Shape::line(&[(0.80, 0.10), (0.30, 0.66)]),
            Shape::Circle {
                at: (0.24, 0.80),
                r: 0.14,
                filled: false,
            },
            Shape::Circle {
                at: (0.76, 0.80),
                r: 0.14,
                filled: false,
            },
        ],
        // One page behind another.
        Icon::Copy => vec![
            Shape::rect(0.10, 0.10, 0.52, 0.60),
            Shape::rect(0.36, 0.30, 0.54, 0.60),
        ],
        // A clipboard: the board, and the clip on top of it.
        Icon::Paste => vec![
            Shape::rect(0.16, 0.18, 0.68, 0.72),
            Shape::rect(0.36, 0.08, 0.28, 0.18),
        ],
        // Two copies side by side, and the arrow that made the second one.
        Icon::Repeat => vec![
            Shape::rect(0.08, 0.28, 0.30, 0.44),
            Shape::rect(0.52, 0.28, 0.30, 0.44),
            Shape::poly(&[(0.78, 0.34), (0.96, 0.50), (0.78, 0.66)]),
        ],
        Icon::Trash => vec![
            Shape::line(&[(0.14, 0.26), (0.86, 0.26)]),
            Shape::closed(&[(0.24, 0.26), (0.76, 0.26), (0.68, 0.90), (0.32, 0.90)]),
            Shape::line(&[(0.38, 0.14), (0.62, 0.14)]),
        ],
        // A speaker, crossed out.
        Icon::Mute => vec![
            Shape::poly(&[
                (0.10, 0.38),
                (0.28, 0.38),
                (0.48, 0.16),
                (0.48, 0.84),
                (0.28, 0.62),
                (0.10, 0.62),
            ]),
            Shape::line(&[(0.62, 0.34), (0.90, 0.66)]),
            Shape::line(&[(0.90, 0.34), (0.62, 0.66)]),
        ],
        // Headphones: the band, and the two cups.
        Icon::Solo => vec![
            Shape::line(&[
                (0.14, 0.66),
                (0.14, 0.46),
                (0.50, 0.16),
                (0.86, 0.46),
                (0.86, 0.66),
            ]),
            Shape::poly(&[(0.06, 0.58), (0.24, 0.58), (0.24, 0.88), (0.06, 0.88)]),
            Shape::poly(&[(0.76, 0.58), (0.94, 0.58), (0.94, 0.88), (0.76, 0.88)]),
        ],
        // A horseshoe magnet, feet down — the snap glyph everywhere.
        Icon::Magnet => vec![
            Shape::line(&[
                (0.18, 0.86),
                (0.18, 0.44),
                (0.50, 0.14),
                (0.82, 0.44),
                (0.82, 0.86),
            ]),
            Shape::line(&[(0.18, 0.68), (0.40, 0.68)]),
            Shape::line(&[(0.60, 0.68), (0.82, 0.68)]),
        ],
        // Two of the same thing, one behind: what an onion skin *is*.
        Icon::Ghost => vec![
            Shape::rect(0.10, 0.24, 0.46, 0.52),
            Shape::rect(0.40, 0.36, 0.46, 0.52),
        ],
        // Three faders at three settings, which is what the panel behind this
        // icon actually looks like.
        Icon::Sliders => vec![
            Shape::line(&[(0.10, 0.24), (0.90, 0.24)]),
            Shape::line(&[(0.10, 0.50), (0.90, 0.50)]),
            Shape::line(&[(0.10, 0.76), (0.90, 0.76)]),
            Shape::poly(&[(0.26, 0.14), (0.36, 0.14), (0.36, 0.34), (0.26, 0.34)]),
            Shape::poly(&[(0.62, 0.40), (0.72, 0.40), (0.72, 0.60), (0.62, 0.60)]),
            Shape::poly(&[(0.38, 0.66), (0.48, 0.66), (0.48, 0.86), (0.38, 0.86)]),
        ],
        // An octave: the white keys, and the black ones over them.
        Icon::Piano => vec![
            Shape::rect(0.08, 0.18, 0.84, 0.64),
            Shape::line(&[(0.36, 0.18), (0.36, 0.82)]),
            Shape::line(&[(0.64, 0.18), (0.64, 0.82)]),
            Shape::poly(&[(0.26, 0.18), (0.46, 0.18), (0.46, 0.54), (0.26, 0.54)]),
            Shape::poly(&[(0.54, 0.18), (0.74, 0.18), (0.74, 0.54), (0.54, 0.54)]),
        ],
        // A line that leaves one thing and arrives at another.
        Icon::Route => vec![
            Shape::line(&[(0.12, 0.24), (0.50, 0.24), (0.50, 0.76), (0.78, 0.76)]),
            Shape::poly(&[(0.72, 0.62), (0.92, 0.76), (0.72, 0.90)]),
            Shape::Circle {
                at: (0.14, 0.24),
                r: 0.10,
                filled: true,
            },
        ],
        Icon::ZoomIn => vec![
            Shape::Circle {
                at: (0.42, 0.42),
                r: 0.30,
                filled: false,
            },
            Shape::line(&[(0.64, 0.64), (0.92, 0.92)]),
            Shape::line(&[(0.26, 0.42), (0.58, 0.42)]),
            Shape::line(&[(0.42, 0.26), (0.42, 0.58)]),
        ],
        Icon::ZoomOut => vec![
            Shape::Circle {
                at: (0.42, 0.42),
                r: 0.30,
                filled: false,
            },
            Shape::line(&[(0.64, 0.64), (0.92, 0.92)]),
            Shape::line(&[(0.26, 0.42), (0.58, 0.42)]),
        ],
        Icon::ArrowUp => vec![
            Shape::line(&[(0.50, 0.90), (0.50, 0.26)]),
            Shape::poly(&[(0.24, 0.36), (0.50, 0.06), (0.76, 0.36)]),
        ],
        Icon::ArrowDown => vec![
            Shape::line(&[(0.50, 0.10), (0.50, 0.74)]),
            Shape::poly(&[(0.24, 0.64), (0.50, 0.94), (0.76, 0.64)]),
        ],
        Icon::Plus => vec![
            Shape::line(&[(0.14, 0.50), (0.86, 0.50)]),
            Shape::line(&[(0.50, 0.14), (0.50, 0.86)]),
        ],
        Icon::Minus => vec![Shape::line(&[(0.14, 0.50), (0.86, 0.50)])],
        Icon::Chevron => vec![Shape::line(&[(0.24, 0.36), (0.50, 0.64), (0.76, 0.36)])],
        // The tab, then the body, in one outline.
        Icon::Folder => vec![Shape::closed(&[
            (0.08, 0.82),
            (0.08, 0.20),
            (0.40, 0.20),
            (0.48, 0.32),
            (0.92, 0.32),
            (0.92, 0.82),
        ])],
        // A page with the corner turned, and the plus that makes it new.
        Icon::NewFile => vec![
            Shape::closed(&[
                (0.18, 0.08),
                (0.60, 0.08),
                (0.80, 0.30),
                (0.80, 0.92),
                (0.18, 0.92),
            ]),
            Shape::line(&[(0.60, 0.08), (0.60, 0.30), (0.80, 0.30)]),
            Shape::line(&[(0.32, 0.62), (0.66, 0.62)]),
            Shape::line(&[(0.49, 0.45), (0.49, 0.79)]),
        ],
    }
}

// ------------------------------------------------------ mouse cursors ---

/// How thick a cursor's stroke is, as a fraction of its size.
const CURSOR_STROKE: f32 = 0.09;

/// And its outline, which is what makes it visible on a light background.
const CURSOR_EDGE: f32 = 0.055;

/// How much of the bitmap the glyph takes, leaving room for the outline.
const CURSOR_INSET: f32 = 0.10;

/// Supersampling factor. Three is enough that a diagonal reads as a line
/// rather than a staircase at 32 pixels, and cheap enough to do once at
/// start-up for every cursor in the set.
const CURSOR_SS: u32 = 3;

/// Draws `icon` into a `size` × `size` RGBA bitmap, for use as a mouse cursor.
///
/// **White with a dark outline**, always — a cursor is drawn over the user's
/// own colours, and a single-ink one disappears against half of them. That is
/// why this does not take a colour.
///
/// Software-rasterised rather than rendered through vello: a cursor is a
/// 32-pixel bitmap made once at start-up, and reaching for the GPU pipeline
/// that draws the window in order to make one would tie the pointer to the
/// surface being alive.
pub fn rasterise(icon: Icon, size: u32) -> Vec<u8> {
    let size = size.max(1);
    let hi = size * CURSOR_SS;
    let scale = hi as f32 * (1.0 - CURSOR_INSET * 2.0);
    let offset = hi as f32 * CURSOR_INSET;
    let at = |p: (f32, f32)| (p.0 * scale + offset, p.1 * scale + offset);

    // Two coverage masks: the glyph, and the thicker outline behind it.
    let mut ink = vec![0u8; (hi * hi) as usize];
    let mut edge = vec![0u8; (hi * hi) as usize];
    let stroke = CURSOR_STROKE * hi as f32;
    let outline = (CURSOR_STROKE + CURSOR_EDGE * 2.0) * hi as f32;

    for (mask, width) in [(&mut edge, outline), (&mut ink, stroke)] {
        for shape in shapes(icon) {
            match shape {
                Shape::Line { points, closed } => {
                    let points: Vec<(f32, f32)> = points.into_iter().map(at).collect();
                    for pair in points.windows(2) {
                        stamp_segment(mask, hi, pair[0], pair[1], width);
                    }
                    if closed && points.len() > 2 {
                        stamp_segment(mask, hi, points[points.len() - 1], points[0], width);
                    }
                }
                Shape::Poly(points) => {
                    let points: Vec<(f32, f32)> = points.into_iter().map(at).collect();
                    fill_polygon(mask, hi, &points);
                    // Its own outline too, so the filled shapes grow by the
                    // same amount the stroked ones do.
                    for pair in points.windows(2) {
                        stamp_segment(mask, hi, pair[0], pair[1], width);
                    }
                    if points.len() > 2 {
                        stamp_segment(mask, hi, points[points.len() - 1], points[0], width);
                    }
                }
                Shape::Circle { at: c, r, filled } => {
                    let centre = at(c);
                    let radius = r * scale;
                    stamp_circle(mask, hi, centre, radius, width, filled);
                }
            }
        }
    }

    // Down-sample both masks together and compose: white glyph over dark edge.
    let mut out = vec![0u8; (size * size * 4) as usize];
    let per = CURSOR_SS * CURSOR_SS;
    for y in 0..size {
        for x in 0..size {
            let (mut ink_sum, mut edge_sum) = (0u32, 0u32);
            for sy in 0..CURSOR_SS {
                for sx in 0..CURSOR_SS {
                    let i = ((y * CURSOR_SS + sy) * hi + x * CURSOR_SS + sx) as usize;
                    ink_sum += u32::from(ink[i]);
                    edge_sum += u32::from(edge[i]);
                }
            }
            let ink_a = (ink_sum / per) as u8;
            let edge_a = (edge_sum / per) as u8;
            let alpha = ink_a.max(edge_a);
            if alpha == 0 {
                continue;
            }
            // Where the glyph is, white; where only the outline is, near-black.
            let value = if ink_a > 96 { 255u8 } else { 24u8 };
            let o = ((y * size + x) * 4) as usize;
            out[o] = value;
            out[o + 1] = value;
            out[o + 2] = value;
            out[o + 3] = alpha;
        }
    }
    out
}

/// A thick line segment, stamped into a coverage mask.
fn stamp_segment(mask: &mut [u8], size: u32, a: (f32, f32), b: (f32, f32), width: f32) {
    let half = (width / 2.0).max(0.5);
    let (min_x, max_x) = (a.0.min(b.0) - half, a.0.max(b.0) + half);
    let (min_y, max_y) = (a.1.min(b.1) - half, a.1.max(b.1) + half);
    for y in bounds(min_y, max_y, size) {
        for x in bounds(min_x, max_x, size) {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            if distance_to_segment(p, a, b) <= half {
                mask[(y * size + x) as usize] = 255;
            }
        }
    }
}

/// A circle, stroked at `width` or filled.
fn stamp_circle(mask: &mut [u8], size: u32, at: (f32, f32), r: f32, width: f32, filled: bool) {
    let half = (width / 2.0).max(0.5);
    let reach = r + half;
    for y in bounds(at.1 - reach, at.1 + reach, size) {
        for x in bounds(at.0 - reach, at.0 + reach, size) {
            let dx = x as f32 + 0.5 - at.0;
            let dy = y as f32 + 0.5 - at.1;
            let d = (dx * dx + dy * dy).sqrt();
            let inside = if filled {
                d <= r
            } else {
                (d - r).abs() <= half
            };
            if inside {
                mask[(y * size + x) as usize] = 255;
            }
        }
    }
}

/// Even-odd scanline fill.
fn fill_polygon(mask: &mut [u8], size: u32, points: &[(f32, f32)]) {
    if points.len() < 3 {
        return;
    }
    let min_y = points.iter().fold(f32::MAX, |a, p| a.min(p.1));
    let max_y = points.iter().fold(f32::MIN, |a, p| a.max(p.1));
    let mut crossings: Vec<f32> = Vec::with_capacity(points.len());
    for y in bounds(min_y, max_y, size) {
        let scan = y as f32 + 0.5;
        crossings.clear();
        for i in 0..points.len() {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            if (a.1 <= scan) == (b.1 <= scan) {
                continue; // the edge does not cross this line
            }
            let t = (scan - a.1) / (b.1 - a.1);
            crossings.push(a.0 + t * (b.0 - a.0));
        }
        crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // In pairs: between an odd crossing and the next is inside the shape.
        // A stray odd one at the end belongs to a vertex the scanline grazed
        // and is dropped.
        for span in crossings.as_chunks::<2>().0 {
            for x in bounds(span[0], span[1], size) {
                mask[(y * size + x) as usize] = 255;
            }
        }
    }
}

/// The pixel range `lo..=hi` covers, clamped to the bitmap.
///
/// A half-open range rather than an inclusive one, so "nothing" is `0..0`
/// rather than a reversed range — which is legal, yields nothing, and reads
/// like a bug to everyone including the linter.
fn bounds(lo: f32, hi: f32, size: u32) -> std::ops::Range<u32> {
    let lo = lo.floor().max(0.0) as u32;
    let hi = (hi.ceil().max(0.0) as u32 + 1).min(size);
    if lo >= hi { 0..0 } else { lo..hi }
}

fn distance_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= f32::EPSILON {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}
