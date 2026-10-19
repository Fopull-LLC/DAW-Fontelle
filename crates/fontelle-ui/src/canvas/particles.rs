//! A deleted note is zapped out of existence: it flashes to light and
//! collapses to a thin line, and fine sparks — white-hot at first — cool to
//! its colour as they drift and fade. Gone in under half a second.
//!
//! Ty, 2026-10-19, of the first version's round flecks: *"it almsot makes it
//! looks like the notes are made out of some sort of chunky material"* — so
//! nothing here is bigger than a couple of pixels except the light itself,
//! and nothing is the note's flat colour until it has cooled.
//!
//! Pure and stepped by time, the way the mixer's cables are: no clock is read
//! here, and the same delete makes the same burst. A big delete bursts from
//! the outline of what went, not from every note — two hundred bursts would
//! be a storm, not a flourish.

use crate::layout::Rect;

/// How long the longest-lived spark lives, in seconds.
pub const PARTICLE_SECONDS: f32 = 0.4;
/// The most bursts one delete makes; past it the outline bursts instead.
pub const MAX_BURSTS: usize = 12;
/// Sparks a burst throws.
const SPARKS: usize = 32;
/// How long the flash takes to collapse and go out.
const FLASH_SECONDS: f32 = 0.2;
/// How white a spark is when it leaves the note, and the flash at its
/// brightest: light, not paint.
const SPARK_HEAT: f32 = 0.85;
const FLASH_HEAT: f32 = 0.75;

/// What a fleck is: the note's own light, or a spark of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FleckKind {
    /// The note itself turned to light, collapsing to a line. The renderer
    /// gives it a soft glow.
    Flash,
    /// A fine spark thrown off it.
    Spark,
}

/// One fleck as it is drawn now: its centre, its size, and its colour with
/// the fade in its alpha. `size` is the smaller side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fleck {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub width: f32,
    pub height: f32,
    pub colour: [u8; 4],
    pub kind: FleckKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Live {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    width: f32,
    height: f32,
    colour: [u8; 4],
    age: f32,
    life: f32,
    kind: FleckKind,
}

/// Every burst still in the air.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Particles {
    live: Vec<Live>,
    bursts: usize,
}

/// A small fixed sequence in 0..1, so a burst is the same every time.
fn wobble(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9).wrapping_add(0x7F4A_7C15);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// `colour` mixed `heat` of the way to white.
fn heated(colour: [u8; 4], heat: f32) -> [u8; 4] {
    let mix = |c: u8| (f32::from(c) + (255.0 - f32::from(c)) * heat).round() as u8;
    [mix(colour[0]), mix(colour[1]), mix(colour[2]), colour[3]]
}

impl Particles {
    /// Bursts each of `notes` (or, when there are more than [`MAX_BURSTS`],
    /// the outline round them all) in `colour`.
    pub fn burst(&mut self, notes: &[Rect], colour: [u8; 4]) {
        let outline;
        let rects: &[Rect] = if notes.len() > MAX_BURSTS {
            let mut r = notes[0];
            for n in &notes[1..] {
                let (x0, y0) = (r.x.min(n.x), r.y.min(n.y));
                let (x1, y1) = (r.right().max(n.right()), r.bottom().max(n.bottom()));
                r = Rect::new(x0, y0, x1 - x0, y1 - y0);
            }
            outline = [r];
            &outline
        } else {
            notes
        };
        for (i, r) in rects.iter().enumerate() {
            if r.is_empty() {
                continue;
            }
            self.bursts += 1;
            let (cx, cy) = (r.x + r.width / 2.0, r.y + r.height / 2.0);
            self.live.push(Live {
                x: cx,
                y: cy,
                vx: 0.0,
                vy: 0.0,
                width: r.width,
                height: r.height,
                colour,
                age: 0.0,
                life: FLASH_SECONDS,
                kind: FleckKind::Flash,
            });
            for k in 0..SPARKS {
                let seed = (i * SPARKS + k) as u32;
                let along = wobble(seed * 7 + 1);
                let across = 0.15 + 0.7 * wobble(seed * 7 + 2);
                let (x, y) = (r.x + r.width * along, r.y + r.height * across);
                // Out from the middle of the note, mostly up and down off a
                // long one, with a little scatter so it isn't a fan.
                let (dx, dy) = (
                    (x - cx) / r.width.max(1.0),
                    (y - cy) / r.height.max(1.0) * 2.0,
                );
                let angle = dy.atan2(dx) + (wobble(seed * 7 + 3) - 0.5) * 1.6;
                let speed = 25.0 + 75.0 * wobble(seed * 7 + 4);
                let size = 1.4 + 1.0 * wobble(seed * 7 + 5);
                self.live.push(Live {
                    x,
                    y,
                    vx: angle.cos() * speed,
                    vy: angle.sin() * speed,
                    width: size,
                    height: size,
                    colour,
                    age: 0.0,
                    life: PARTICLE_SECONDS * (0.6 + 0.4 * wobble(seed * 7 + 6)),
                    kind: FleckKind::Spark,
                });
            }
        }
    }

    /// Moves everything on by `dt` seconds: sparks slow and rise a little,
    /// like ash off a flame; the spent ones go.
    pub fn step(&mut self, dt: f32) {
        for f in &mut self.live {
            f.age += dt;
            f.x += f.vx * dt;
            f.y += f.vy * dt;
            let drag = 1.0 - (4.0 * dt).min(1.0);
            f.vx *= drag;
            f.vy = f.vy * drag - 40.0 * dt;
        }
        self.live.retain(|f| f.age < f.life);
        if self.live.is_empty() {
            self.bursts = 0;
        }
    }

    /// The flecks as they are drawn now.
    pub fn flecks(&self) -> Vec<Fleck> {
        self.live
            .iter()
            .map(|f| {
                let u = (f.age / f.life).clamp(0.0, 1.0);
                let left = 1.0 - u;
                let (width, height, colour, fade) = match f.kind {
                    // Collapses to a line a little wider than the note was,
                    // and goes out.
                    FleckKind::Flash => (
                        f.width * (1.0 + 0.15 * u),
                        (f.height * left.powf(1.5)).max(1.0),
                        heated(f.colour, FLASH_HEAT + (1.0 - FLASH_HEAT) * u),
                        left.sqrt(),
                    ),
                    // Cools from white to the note's colour, and fades
                    // late: bright for most of its flight.
                    FleckKind::Spark => (
                        f.width,
                        f.height,
                        heated(f.colour, SPARK_HEAT * left * left),
                        left.sqrt(),
                    ),
                };
                let mut colour = colour;
                colour[3] = (f32::from(colour[3]) * fade) as u8;
                Fleck {
                    x: f.x,
                    y: f.y,
                    size: width.min(height),
                    width,
                    height,
                    colour,
                    kind: f.kind,
                }
            })
            .collect()
    }

    /// Whether anything is still in the air — what keeps frames coming.
    pub fn is_moving(&self) -> bool {
        !self.live.is_empty()
    }

    /// How many bursts are in the air.
    pub fn bursts(&self) -> usize {
        self.bursts
    }
}
